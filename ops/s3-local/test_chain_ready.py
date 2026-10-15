import unittest
from unittest.mock import patch
import chain_ready as target
import test_chain_preflight as fixture

class ChainReadyTest(fixture.ChainPreflightTest):
    # Only collect this class's new tests in the recorded invocation.
    def body(self, ready='sys.stdout.write("CHAIN_READY\\n");sys.stdout.flush()'):
        return ('import sys,time,os\n'
                'if sys.argv[1]=="preflight":\n'
                ' sys.stdout.buffer.write('+repr(target.chain_preflight.EXPECTED)+');sys.exit(0)\n'
                'assert sys.argv[1]=="start"\n'
                'assert not any(k.startswith("PAPERCLIP_") for k in os.environ)\n'+ready+'\n'
                'assert sys.stdin.buffer.read()==b""\n')
    def probe(self, staged, audit=lambda:{}, **kw):
        return target.ready(staged,'a'*64,self.root/'home','127.0.0.1:26657',
                            '127.0.0.1:26656','',audit,**kw)
    def test_ready_no_start_reap(self):
        children=[];real=target.subprocess.Popen
        def spawn(*a,**kw):
            p=real(*a,**kw);children.append(p);return p
        pids=[]
        with patch.object(target.subprocess,'Popen',side_effect=spawn):
            with self.probe(self.staged(self.body()),on_spawn=pids.append) as report:
                self.assertTrue(report['ready']);self.assertFalse(report['start_sent'])
        self.assertEqual(pids,[children[1].pid])
        for p in children:
            self.assertIsNotNone(p.returncode)
            self.assertTrue(p.stdout.closed and p.stderr.closed)
        self.assertTrue(children[1].stdin.closed)
    def test_audit_mutation_and_interrupt(self):
        for failure in ('changed','bytes','interrupt','stop'):
            staged=self.staged(self.body());calls=[0];stop=[False]
            def audit():
                calls[0]+=1
                if calls[0]==3:
                    if failure=='changed': return {'changed':True}
                    if failure=='bytes': self.input.write_bytes(b'changed')
                    if failure=='interrupt': raise KeyboardInterrupt()
                    if failure=='stop': stop[0]=True
                return {}
            with self.subTest(failure=failure), self.assertRaises(KeyboardInterrupt if failure=='interrupt' else ValueError):
                with self.probe(staged,audit,stopped=lambda:stop[0]): self.fail('yielded')
    def test_protocol_timeout_and_spawn_callback(self):
        for body in ('print("WRONG",flush=True)','sys.stderr.write("secret");sys.stderr.flush()',
                     'time.sleep(10)'):
            with self.subTest(body=body),self.assertRaises(ValueError):
                with self.probe(self.staged(self.body(body)),timeout=.3): self.fail('yielded')
        def fail(pid): raise RuntimeError('callback')
        with self.assertRaises(RuntimeError):
            with self.probe(self.staged(self.body()),on_spawn=fail): self.fail('yielded')
    def test_denial_before_spawn(self):
        staged=self.staged(self.body())
        with patch.object(target.subprocess,'Popen') as spawn:
            for limit in (True,0,31,float('nan')):
                with self.assertRaises(ValueError):
                    with self.probe(staged,timeout=limit): self.fail('yielded')
            with self.assertRaises(ValueError):
                with self.probe(staged,stopped=lambda:True): self.fail('yielded')
            spawn.assert_not_called()

    def test_signal_denial_still_reaps(self):
        from contextlib import ExitStack
        staged=self.staged(self.body())
        real=target.subprocess.Popen;children=[]
        def spawn(*a,**kw):
            p=real(*a,**kw);children.append(p);return p
        with patch.object(target.subprocess,'Popen',side_effect=spawn):
            with ExitStack() as mocks:
                with self.assertRaises(PermissionError):
                    with self.probe(staged):
                        mocks.enter_context(patch.object(target.os,'killpg',side_effect=PermissionError('denied')))
        self.assertIsNotNone(children[-1].returncode)
        self.assertTrue(children[-1].stdout.closed and children[-1].stderr.closed)
