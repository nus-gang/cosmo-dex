import contextlib,json,os,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
from bootstrap_diagnostic import diagnostic
import fetch_process as f
import bootstrap_initialize_cli as cli
import bootstrap_initialize as init
import bootstrap_fetch_cli as fetch_cli
import test_bootstrap_cli as fixture

class DiagnosticTest(unittest.TestCase):
    def setUp(self):
        t=tempfile.TemporaryDirectory();self.addCleanup(t.cleanup);self.root=Path(t.name).resolve()
    def events(self):return [json.loads(x) for x in (self.root/'bootstrap-diagnostic.jsonl').read_text().splitlines()]
    def test_no_replace_private_identity_and_closed_vocabulary(self):
        with diagnostic(self.root) as trace:
            trace.emit('checker',capture_sha256='a'*64)
            with self.assertRaises(ValueError):trace.emit('failed',code='Bearer SECRET')
            with self.assertRaises(ValueError):trace.emit('sent',secret='not allowed')
            (self.root/'snapshot-fetch.wire').rename(self.root/'original-wire')
            (self.root/'snapshot-fetch.wire').write_bytes(b'changed')
            with self.assertRaises(ValueError):trace.wire(b'data')
        original=(self.root/'bootstrap-diagnostic.jsonl').read_bytes()
        with self.assertRaises(FileExistsError):
            with diagnostic(self.root):pass
        self.assertEqual((self.root/'bootstrap-diagnostic.jsonl').read_bytes(),original)
        self.assertNotIn(b'SECRET',original)
        self.assertEqual((self.root/'bootstrap-diagnostic.jsonl').stat().st_mode&0o777,0o600)
    def test_different_failures_have_fixed_stage_and_create_zero(self):
        for phase in ('initial_audit','checker','transport','persist_entity'):
            root=self.root/phase;root.mkdir(mode=0o700)
            args=fixture.BootstrapCliTest.args(self)+['--chain-rpc','127.0.0.1:26657','--evidence-root',str(root),'--home',str(root/'home')]
            def fetch(*args,trace,**kw):
                trace.emit('checker')
                if phase=='checker':raise ValueError('secret checker diagnostic')
                trace.emit('request_start');trace.emit('sent',sent_bytes=123)
                trace.wire(b'HTTP/1.1 200 OK\r\npartial')
                if phase=='transport':raise f.FetchFailure('FETCH_REJECTED')
                return b'raw'
            def audit(*a):
                if phase=='initial_audit':raise ValueError('secret bearer')
                return {}
            with patch.object(init.approval_gate,'inspect',side_effect=audit),patch.object(init,'verify_input_set',return_value=(b'capture',{})),patch.object(fetch_cli,'fetch',side_effect=fetch),patch.object(init,'stage') as stage,patch.object(init,'run') as create:
                with contextlib.ExitStack() as stack:
                    if phase=='persist_entity':
                        original=fetch_cli.os.fsync
                        def sync(fd):
                            if os.fstat(fd).st_size==3:raise OSError('secret fsync')
                            return original(fd)
                        stack.enter_context(patch.object(fetch_cli.os,'fsync',side_effect=sync))
                    import io
                    out,err=io.StringIO(),io.StringIO()
                    with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):self.assertEqual(cli.main(args),2)
                create.assert_not_called();stage.assert_not_called()
                self.assertFalse((root/'home').exists());self.assertEqual(out.getvalue(),'')
                raw=(root/'bootstrap-diagnostic.jsonl').read_text();self.assertNotIn('secret',raw)
                self.assertEqual(json.loads(raw.splitlines()[-1])['phase'],phase if phase!='transport' else 'sent')
                self.assertEqual(err.getvalue(),'LOCAL_INITIALIZE_REJECTED_PRESERVE_HOME_AND_EVIDENCE\n')
    def test_real_child_pipe_wire_and_fixed_error_without_stderr_leak(self):
        child=self.root/'child'
        child.write_text('#!'+sys.executable+'\nimport sys,os,json\nsys.stdin.buffer.read()\n'
            'd=int(sys.argv[5]);w=int(sys.argv[7])\n'
            'for stage,code,sent,received in [("request_start","OK",0,0),("sent","OK",321,0),("received","OK",321,7),("failed","QUERY_LENGTH",321,7)]:\n'
            ' os.write(d,(json.dumps(dict(stage=stage,code=code,sent_bytes=sent,received_bytes=received,child_elapsed_ns=1))+"\\n").encode())\n'
            'os.write(w,b"partial")\nos.write(2,b"Bearer NEVER_LOG_ME")\nsys.exit(2)\n')
        child.chmod(0o500)
        with self.assertRaises(f.FetchFailure),diagnostic(self.root) as trace:
            f.fetch_captured(child,'127.0.0.1:26657','a'*64,self.root/'profile',True,b'input',timeout=3,trace=trace)
        self.assertEqual((self.root/'snapshot-fetch.wire').read_bytes(),b'partial')
        events=self.events();self.assertTrue(any(e['code']=='QUERY_LENGTH' for e in events))
        self.assertTrue(any(e['stage']=='child_exit' and e['exit_code']==2 for e in events))
        self.assertNotIn('NEVER_LOG_ME',(self.root/'bootstrap-diagnostic.jsonl').read_text())
        self.assertEqual(sum(e['stage']=='child_spawn' for e in events),1)
    def test_fsync_failure_never_starts_child_and_preserves_reservation(self):
        with patch('bootstrap_diagnostic.os.fsync',side_effect=OSError('secret')),patch.object(f.subprocess,'Popen') as spawn:
            with self.assertRaises(OSError):
                with diagnostic(self.root):f.fetch_captured(None,None,None,None,None,None)
            spawn.assert_not_called()
        self.assertTrue((self.root/'bootstrap-diagnostic.jsonl').exists())

if __name__=='__main__':unittest.main()
