import contextlib
import io
import json
import subprocess
import sys
import unittest
from unittest.mock import patch

import receipt_apply_cli as cli
import receipt_apply_ready as ready_target
import receipt_apply_run as run_target
import test_ready_worker as ready_fixtures
import test_reviewed_cli as cli_fixtures


class ManagedF09Test(unittest.TestCase):
    setUp = ready_fixtures.ReadyTest.setUp
    worker = ready_fixtures.ReadyTest.worker
    reaped = ready_fixtures.ReadyTest.reaped

    def selection(self):
        return dict(batch_id='ab'*32, evidence_root=self.root/'evidence', enable=True)

    def test_ready_uses_exact_argv_without_start_and_reaps(self):
        expected = ready_target.receipt_apply_arguments(**self.selection())
        staged = self.worker('assert sys.argv[1]=="f09-crash-captured"\n'
            'assert sys.argv[6:]=='+repr(expected+['--worker-input','exact'])+'\n'
            's.sendall(b"READY\\n")\n'
            f'if s.recv(16): open({str(self.root/"started")!r},"w").write("BAD")\n')
        calls=[]
        with ready_target.ready(staged,['--worker-input','exact'],lambda:calls.append(1) or 'same',
                                timeout=2,**self.selection()) as report:
            self.assertEqual(report['F09_verified'],False)
            self.assertEqual(report['apply_called_verified'],False)
        self.assertEqual(len(calls),2); self.reaped()
        self.assertFalse((self.root/'started').exists())
        self.assertFalse((self.root/'evidence').exists())

    def test_options_and_mixed_modes_reject_before_spawn(self):
        staged=self.worker('time.sleep(2)\n')
        for change in ({'enable':False},{'batch_id':'A'*64},{'batch_id':'a'*63},
                       {'evidence_root':'relative'},{'evidence_root':'/a/../b'}):
            values=self.selection(); values.update(change)
            with self.assertRaises(ValueError), ready_target.ready(staged,[],lambda:'same',**values):
                self.fail('yielded')
        for arg in ('--enable-f05-before-send','--enable-f14-prepare','--enable-storage-fault',
                    '--fault-errno','--batch-id','--worker-inputs',None):
            with patch.object(ready_target,'_ready_scope') as scope:
                with self.assertRaisesRegex(ValueError,'DUPLICATE_F09_OPTIONS'), ready_target.ready(
                        staged,[arg],lambda:'same',**self.selection()): self.fail('yielded')
                scope.assert_not_called()

    def test_run_exit86_is_unknown_and_start_denials_preserve_no_evidence(self):
        staged=self.worker('assert sys.argv[1]=="f09-crash-captured"\n'
            's.sendall(b"READY\\n")\nassert s.recv(16)==b"START\\n"\nassert s.recv(1)==b""\n'
            f'open({str(self.root/"evidence")!r},"wb").write(b"preserve")\nos._exit(86)\n')
        report=run_target.run(staged,[],lambda:'same',timeout=2,**self.selection())
        self.assertEqual(report['outcome'],'UNKNOWN'); self.assertEqual(report['child_exit'],86)
        self.assertFalse(report['apply_called_verified']); self.assertFalse(report['F09_verified'])
        self.assertFalse(report['durable_ack']); self.assertEqual(report['DEV'],'NOT_RUN')
        self.reaped(); self.assertEqual((self.root/'evidence').read_bytes(),b'preserve')
        (self.root/'evidence').unlink()
        for mode in ('revoke','binary','capture','stop','interrupt'):
            staged=self.worker('s.sendall(b"READY\\n")\ns.recv(16)\n')
            calls=[]; stopped=[False]
            def audit():
                calls.append(1)
                if len(calls)==3:
                    if mode=='revoke': return 'changed'
                    if mode=='binary': staged.executable.chmod(0o700); staged.executable.write_bytes(b'changed')
                    if mode=='capture': object.__setattr__(staged,'capture',b'changed')
                    if mode=='stop': stopped[0]=True
                    if mode=='interrupt': raise KeyboardInterrupt()
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)):
                run_target.run(staged,[],audit,stop=lambda:stopped[0],timeout=2,**self.selection())
            self.reaped(); self.assertFalse((self.root/'evidence').exists())

    def cli_args(self, command='run-reviewed'):
        return [command,*cli_fixtures.ReviewedCliTest.args(self),
                '--enable-f09-receipt-apply','--batch-id','a'*64,
                '--fault-evidence-root','/private/f09-evidence']

    def invoke(self,args):
        out,err=io.StringIO(),io.StringIO()
        with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err): code=cli.main(args)
        return code,out.getvalue(),err.getvalue()

    def test_authenticated_cli_exact_scope_and_report_validation(self):
        expected={'child_result':None,'child_exit':86,'fault_started':True,'outcome':'UNKNOWN',
            'apply_called_verified':False,'crash_verified':False,'receipt_durability_verified':False,
            'approval_verified':False,'reusable_permit':False,'replay_verified':False,
            'durable_ack':False,'DEV':'NOT_RUN','F09_verified':False}
        events=[]
        @contextlib.contextmanager
        def stage(*args):
            self.assertNotIn('--batch-id',args[9]); events.append('stage')
            try: yield 'staged'
            finally: events.append('clean')
        def run(staged,args,audit,**kw):
            self.assertEqual((kw['batch_id'],kw['evidence_root'],kw['enable']),
                             ('a'*64,'/private/f09-evidence',True))
            audit(); events.append('run'); return expected
        with patch.object(cli,'stage',side_effect=stage),patch.object(cli,'run',side_effect=run),\
             patch.object(cli.approval_gate,'inspect',return_value={}):
            code,out,err=self.invoke(self.cli_args())
        self.assertEqual((code,err),(0,'')); self.assertEqual(json.loads(out),expected)
        self.assertEqual(events,['stage','run','clean'])
        for report in (dict(expected,F09_verified=True),dict(expected,child_exit=True),dict(expected,extra=False)):
            with patch.object(cli,'stage',return_value=contextlib.nullcontext('staged')),\
                 patch.object(cli,'run',return_value=report):
                self.assertEqual(self.invoke(self.cli_args()),(2,'','LOCAL_F09_RUN_REJECTED_OUTCOME_UNKNOWN\n'))
        process=subprocess.Popen([sys.executable,'-B',cli.__file__,'run-reviewed'],stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(process.wait(timeout=3),2); self.assertEqual(process.stdout.read(),b'')
            self.assertEqual(process.stderr.read(),b'LOCAL_F09_RUN_REJECTED_OUTCOME_UNKNOWN\n')
        finally:
            if process.poll() is None: process.kill()
            process.wait()
            for stream in (process.stdin,process.stdout,process.stderr): stream.close()


if __name__ == '__main__': unittest.main()
