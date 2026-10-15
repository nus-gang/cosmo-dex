import hashlib
import unittest
import bootstrap_run as target
import test_bootstrap_ready as fixtures
from bootstrap_stage import StagedBootstrap

class BootstrapRunTest(fixtures.BootstrapReadyTest):
    def creator_run(self, after=''):
        action = ('s.sendall(b"READY\\n")\n'
            'assert s.recv(16)==b"START\\n"\nassert s.recv(1)==b""\n'
            f'open({str(self.root/"started")!r},"wb").write(b"once")\n'
            f'open({str(self.root/"evidence")!r},"wb").write(b"preserved")\n'+after)
        worker=self.worker(action)
        rpc=self.root/'rpc';rpc.write_bytes(b'rpc');rpc.chmod(0o600)
        return StagedBootstrap(worker.executable,worker.executable_sha256,worker.capture,
            worker.capture_sha256,rpc,hashlib.sha256(b'rpc').hexdigest())

    def run_create(self, staged, audit=lambda:'same', **kwargs):
        return target.run(staged,self.root/'home',self.root/'evidence','a'*64,
                          self.root/'profile',audit,**kwargs)

    def test_exact_start_report_reap(self):
        staged=self.creator_run('sys.stdout.buffer.write('+repr(target.SUCCESS)+')\n')
        calls=[]
        report=self.run_create(staged,lambda:calls.append(1) or 'same')
        self.assertEqual(len(calls),3);self.assertTrue(report['child_reported_home_created'])
        self.assertFalse(report['replay_verified']);self.assertFalse(report['reusable_permit'])
        self.reaped();self.assertEqual((self.root/'started').read_bytes(),b'once')

    def test_last_audit_denial_mutation_stop_interrupt_start_zero(self):
        for mode in ['revoke','rpc','binary','stop','interrupt']:
            staged=self.creator_run();calls=[];stopped=[False]
            def audit():
                calls.append(1)
                if len(calls)==3:
                    if mode=='revoke':return 'changed'
                    if mode=='rpc':staged.rpc_file.write_bytes(b'changed')
                    if mode=='binary':
                        staged.executable.chmod(0o700);staged.executable.write_bytes(b'changed')
                    if mode=='stop':stopped[0]=True
                    if mode=='interrupt':raise KeyboardInterrupt()
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)):
                self.run_create(staged,audit,stop=lambda:stopped[0])
            self.reaped();self.assertFalse((self.root/'started').exists())

    def test_failed_output_exit_timeout_preserve_unknown(self):
        for after in ['print("secret",flush=True)\n','sys.exit(2)\n',
                      'time.sleep(2)\n','sys.stdout.buffer.write('+repr(target.SUCCESS+b'x')+')\n']:
            staged=self.creator_run(after)
            with self.assertRaisesRegex(ValueError,'OUTCOME_UNKNOWN'):
                self.run_create(staged,lifetime=.2)
            self.reaped();self.assertEqual((self.root/'evidence').read_bytes(),b'preserved')

    def test_invalid_lifetime_no_child(self):
        staged=self.creator_run()
        for value in [0,61,True,float('nan')]:
            with self.assertRaisesRegex(ValueError,'LIFETIME_LIMIT'):
                self.run_create(staged,lifetime=value)
        self.assertFalse(self.pidfile.exists())

    def test_post_start_stop_interrupt_preserves(self):
        for interrupt in [False,True]:
            marker=self.root/'started'
            if marker.exists():marker.unlink()
            staged=self.creator_run('time.sleep(2)\n')
            def stop():
                if marker.exists():
                    if interrupt:raise KeyboardInterrupt()
                    return True
                return False
            with self.assertRaises((ValueError,KeyboardInterrupt)):
                self.run_create(staged,stop=stop)
            self.reaped();self.assertTrue(marker.exists())
            self.assertEqual((self.root/'evidence').read_bytes(),b'preserved')
