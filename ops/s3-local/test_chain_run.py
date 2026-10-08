import unittest
from unittest.mock import patch
import chain_run as target
import test_chain_preflight as fixture

class ChainRunTest(fixture.ChainPreflightTest):
    def body(self, after='pass'):
        return ('import sys,os,time\n'
                'if sys.argv[1]=="preflight":\n'
                ' sys.stdout.buffer.write('+repr(fixture.target.EXPECTED)+');sys.exit(0)\n'
                'print("CHAIN_READY",flush=True)\n'
                'gate=sys.stdin.buffer.read()\n'
                'if not gate: sys.exit(0)\n'
                'assert gate==b"START\\n"\n'
                'assert not any(k.startswith("PAPERCLIP_") for k in os.environ)\n'+after+'\n')
    def run_chain(self, staged, audit=lambda:{}, **kw):
        return target.run(staged,'a'*64,self.root/'home','127.0.0.1:26657',
                          '127.0.0.1:26656','',audit,**kw)
    def test_start_once_eof_and_reap(self):
        children=[];real=fixture.target.subprocess.Popen
        def spawn(*a,**kw):
            p=real(*a,**kw);children.append(p);return p
        with patch.object(fixture.target.subprocess,'Popen',side_effect=spawn):
            report=self.run_chain(self.staged(self.body('print("ok");sys.stderr.write("log")')))
        self.assertTrue(report['start_sent']);self.assertEqual(report['child_exit'],0)
        self.assertEqual(report['output_bytes'],6)
        self.assertFalse(report['cleanup_complete_verified'])
        for child in children:
            self.assertIsNotNone(child.returncode)
            self.assertTrue(child.stdout.closed and child.stderr.closed and (child.stdin is None or child.stdin.closed))
    def test_fourth_audit_denies_before_start(self):
        marker=self.root/'started'
        for failure in ('approval','bytes','stop','interrupt'):
            calls=[0];stop=[False]
            staged=self.staged(self.body('open('+repr(str(marker))+',"w").write("started")'))
            def audit():
                calls[0]+=1
                if calls[0]==4:
                    if failure=='approval': return {'changed':True}
                    if failure=='bytes': self.profile.write_bytes(b'changed')
                    if failure=='stop': stop[0]=True
                    if failure=='interrupt': raise KeyboardInterrupt()
                return {}
            with self.subTest(failure=failure),self.assertRaises(KeyboardInterrupt if failure=='interrupt' else ValueError):
                self.run_chain(staged,audit,stopped=lambda:stop[0])
            self.assertEqual(calls[0],4);self.assertFalse(marker.exists())
    def test_failure_limits_stop_interrupt_reap(self):
        children=[];real=fixture.target.subprocess.Popen
        def spawn(*a,**kw):
            p=real(*a,**kw);children.append(p);return p
        marker=self.root/'started'
        for failure in ('exit','output','timeout','stop','interrupt'):
            if marker.exists(): marker.unlink()
            after={'exit':'sys.exit(2)','output':'print("x"*2000)',
                   'timeout':'time.sleep(10)','stop':'time.sleep(10)',
                   'interrupt':'time.sleep(10)'}[failure]
            staged=self.staged(self.body('open('+repr(str(marker))+',"w").write("started")\n'+after))
            def stop():
                if marker.exists():
                    if failure=='interrupt': raise KeyboardInterrupt()
                    if failure=='stop': return True
                return False
            with patch.object(fixture.target.subprocess,'Popen',side_effect=spawn):
                if failure=='stop': self.assertTrue(self.run_chain(staged,stopped=stop)['stop_requested'])
                else:
                    with self.assertRaises(KeyboardInterrupt if failure=='interrupt' else ValueError):
                        self.run_chain(staged,stopped=stop,lifetime=.3,output_limit=100)
            self.assertTrue(marker.exists())
            self.assertIsNotNone(children[-1].returncode)
            self.assertTrue(children[-1].stdout.closed and children[-1].stderr.closed)
    def test_invalid_limits_spawn_zero(self):
        staged=self.staged(self.body())
        with patch.object(fixture.target.subprocess,'Popen') as spawn:
            for value in (True,0,3601,float('nan')):
                with self.assertRaises(ValueError): self.run_chain(staged,lifetime=value)
            for value in (True,0,1024*1024+1,1.5):
                with self.assertRaises(ValueError): self.run_chain(staged,output_limit=value)
            spawn.assert_not_called()
