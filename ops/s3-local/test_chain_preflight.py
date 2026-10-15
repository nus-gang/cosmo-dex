import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import chain_preflight as target
from chain_stage import StagedChain

class ChainPreflightTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = self.root/'chain'
        self.input = self.root/'input.json'
        self.profile = self.root/'profile.json'
        self.input.write_bytes(b'input')
        self.profile.write_bytes(b'profile')

    def staged(self, body):
        self.binary.chmod(0o700) if self.binary.exists() else None
        self.binary.write_text('#!'+sys.executable+'\n'+body)
        self.binary.chmod(0o500)
        digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
        return StagedChain(self.binary,self.input,self.profile,digest(self.binary),
                           digest(self.input),digest(self.profile))

    def check(self, staged, **kw):
        return target.check(staged,'a'*64,self.root/'home','127.0.0.1:26657',
                            '127.0.0.1:26656', **kw)

    def test_exact_argv_eof_environment_report(self):
        staged = self.staged('import sys,os\n'
            'assert sys.stdin.buffer.read()==b""\n'
            'assert not any(k.startswith("PAPERCLIP_") for k in os.environ)\n'
            'assert sys.argv[1:]=='+repr(['preflight','--local-demo-profile',str(self.profile),
            '--acknowledge-unproven-space','--input-set',str(self.input),'--runtime-pin','a'*64,
            '--home',str(self.root/'home'),'--rpc','127.0.0.1:26657','--p2p','127.0.0.1:26656'])+'\n'
            'sys.stdout.buffer.write('+repr(target.EXPECTED)+')\n')
        self.assertEqual(self.check(staged), {'b_preflight':True,'approval_verified':False,
                         'service_started':False,'durable_ack':False})
        self.assertFalse((self.root/'home').exists())

    def test_output_exit_stderr_timeout_reaped(self):
        bodies = ['print("wrong")', 'import sys;sys.exit(2)',
                  'import sys;sys.stderr.write("secret")', 'print("x"*5000)',
                  'import time;time.sleep(10)']
        real = target.subprocess.Popen
        children = []
        def track(*args, **kwargs):
            child = real(*args, **kwargs)
            children.append(child)
            return child
        for body in bodies:
            with self.subTest(body=body), patch.object(target.subprocess,'Popen', side_effect=track) as spawn:
                with self.assertRaises(ValueError): self.check(self.staged(body),timeout=0.2)
                self.assertEqual(spawn.call_count,1)
                self.assertIsNotNone(children[-1].returncode)
                self.assertTrue(children[-1].stdout.closed)
        self.assertFalse((self.root/'home').exists())

    def test_bytes_stop_and_arguments_before_child(self):
        staged = self.staged('pass')
        with patch.object(target.subprocess,'Popen') as spawn:
            with self.assertRaisesRegex(ValueError,'STOPPED'): self.check(staged,stopped=lambda:True)
            for limit in [True,0,float('nan'),61]:
                with self.assertRaises(ValueError): self.check(staged,timeout=limit)
            self.input.write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'BYTES_CHANGED'): self.check(staged)
            spawn.assert_not_called()

    def test_success_mutation_interrupt_and_stop_reaped(self):
        staged = self.staged('from pathlib import Path\nimport sys\nPath('+repr(str(self.input))+').write_bytes(b"changed")\nsys.stdout.buffer.write('+repr(target.EXPECTED)+')')
        with self.assertRaisesRegex(ValueError,'BYTES_CHANGED'): self.check(staged)
        staged = self.staged('import time;time.sleep(10)')
        real = target.subprocess.Popen
        children=[]
        def spawn(*a,**kw):
            p=real(*a,**kw);children.append(p);return p
        for interrupt in [False,True]:
            calls=[0]
            def stop():
                calls[0]+=1
                if calls[0]>1:
                    if interrupt: raise KeyboardInterrupt()
                    return True
                return False
            with patch.object(target.subprocess,'Popen',side_effect=spawn):
                with self.assertRaises(KeyboardInterrupt if interrupt else ValueError):
                    self.check(staged,stopped=stop)
            self.assertIsNotNone(children[-1].returncode)
            self.assertTrue(children[-1].stdout.closed)

if __name__=='__main__': unittest.main()
