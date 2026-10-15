import hashlib
import os
import unittest
from bootstrap_stage import StagedBootstrap
import bootstrap_ready as target
import test_ready_worker as fixtures

class BootstrapReadyTest(fixtures.ReadyTest):
    # Only explicit new tests are run; inherited regression suite is run separately.
    def creator(self):
        home=self.root/'home'; evidence=self.root/'evidence'
        action = ('assert sys.argv[1]=="create-captured"\n'
            'assert sys.argv[4:12]=='+repr(['--home',str(home),'--evidence-root',str(evidence),
              '--rpc-file',str(self.root/'rpc'),'--rpc-sha256',hashlib.sha256(b'rpc').hexdigest()])+'\n'
            'assert sys.argv[12]=="--capture-sha256"\n'
            'assert sys.argv[14:]=='+repr(['--runtime-pin','a'*64,'--local-demo-profile',str(self.root/'profile'),
             '--acknowledge-unproven-space'])+'\n'
            f'open({str(evidence)!r},"wb").write(b"preserved")\n'
            's.sendall(b"READY\\n")\ns.recv(16)\n')
        worker=self.worker(action)
        rpc=self.root/'rpc'; rpc.write_bytes(b'rpc'); rpc.chmod(0o600)
        return StagedBootstrap(worker.executable,worker.executable_sha256,worker.capture,
            worker.capture_sha256,rpc,hashlib.sha256(b'rpc').hexdigest())

    def probe(self, staged, audit=lambda:'same', **kwargs):
        return target.ready(staged,self.root/'home',self.root/'evidence','a'*64,
                            self.root/'profile',audit,**kwargs)

    def test_create_argv_ready_denial_preserves_and_reaps(self):
        staged=self.creator(); calls=[]
        with self.probe(staged,lambda:calls.append(1) or 'same') as report:
            self.assertFalse(report['home_created']); self.assertFalse(report['reusable_permit'])
            self.assertEqual(len(calls),2)
        self.reaped(); self.assertFalse((self.root/'home').exists())
        self.assertEqual((self.root/'evidence').read_bytes(),b'preserved')

    def test_create_revoke_rpc_mutation_stop_interrupt_reap(self):
        for mode in ['revoke','rpc','stop','interrupt']:
            staged=self.creator(); calls=[]; stopped=[False]
            def audit():
                calls.append(1)
                if len(calls)==2:
                    if mode=='revoke': return 'changed'
                    if mode=='rpc': staged.rpc_file.write_bytes(b'changed')
                    if mode=='stop': stopped[0]=True
                    if mode=='interrupt': raise KeyboardInterrupt()
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)), self.probe(staged,audit,stop=lambda:stopped[0]):
                self.fail('yielded')
            self.reaped(); self.assertFalse((self.root/'home').exists())
            self.assertEqual((self.root/'evidence').read_bytes(),b'preserved')

    def test_create_invalid_or_changed_rpc_spawn_zero(self):
        staged=self.creator(); staged.rpc_file.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError,'BOOTSTRAP_RPC_CHANGED'), self.probe(staged): self.fail('yielded')
        self.assertFalse(self.pidfile.exists())
        with self.assertRaisesRegex(ValueError,'INPUT_PATH'), target.ready(staged,'relative',self.root,'a'*64,self.root,lambda:None): self.fail('yielded')
        self.assertFalse(self.pidfile.exists())
