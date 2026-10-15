"""IPC-only synthetic children: no Rust service, listener, RPC or real approvals."""
import unittest
from test_ready_worker import ReadyTest
from ready_worker import run_managed

class ManagedTest(unittest.TestCase):
    setUp = ReadyTest.setUp
    worker = ReadyTest.worker
    reaped = ReadyTest.reaped

    def test_exact_start_eof_and_clean_exit(self):
        calls=[]
        staged=self.worker('s.sendall(b"READY\\n")\nassert s.recv(16)==b"START\\n"\nassert s.recv(16)==b""\n')
        report=run_managed(staged,[],lambda: calls.append(1) or 'same',lambda:False,2,1)
        self.assertEqual(len(calls),3)
        self.assertEqual(report,{'worker_exit':0,'reusable_permit':False})
        self.reaped()

    def test_final_audit_denial_mutation_interrupt_start_zero(self):
        marker=self.root/'started'
        for mode in ['changed','revoked','mutated','interrupt']:
            staged=self.worker('s.sendall(b"READY\\n")\nraw=s.recv(16)\n'
                               f'if raw: open({str(marker)!r},"w").write("started")\n')
            calls=[]
            def audit():
                calls.append(1)
                if len(calls)==3:
                    if mode=='changed': return 'different'
                    if mode=='revoked': raise ValueError('secret')
                    if mode=='interrupt': raise KeyboardInterrupt()
                    staged.executable.chmod(0o700)
                    staged.executable.write_bytes(b'changed')
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)):
                run_managed(staged,[],audit,lambda:False,2,1)
            self.assertFalse(marker.exists())
            self.reaped()

    def test_post_start_timeout_output_nonzero_and_stop_reap(self):
        prefix='s.sendall(b"READY\\n")\nassert s.recv(16)==b"START\\n"\nassert s.recv(16)==b""\n'
        for action, expected in [('time.sleep(2)\n','WORKER_LIFETIME_EXCEEDED'),
                                 ('print("secret",flush=True)\ntime.sleep(2)\n','WORKER_UNEXPECTED_OUTPUT'),
                                 ('sys.exit(2)\n','WORKER_FAILED')]:
            staged=self.worker(prefix+action)
            with self.assertRaisesRegex(ValueError,expected):
                run_managed(staged,[],lambda:'same',lambda:False,2,.2)
            self.reaped()
        marker=self.root/'started'
        staged=self.worker(prefix+f'open({str(marker)!r},"w").write("started")\ntime.sleep(2)\n')
        def stop():
            return marker.exists()
        with self.assertRaisesRegex(ValueError,'WORKER_STOPPED'):
            run_managed(staged,[],lambda:'same',stop,2,1)
        self.reaped()

    def test_stop_before_start_and_invalid_limits_spawn_zero(self):
        staged=self.worker('time.sleep(2)\n')
        for limit in [0,-1,301,float('nan'),True]:
            with self.assertRaises(ValueError):
                run_managed(staged,[],lambda:'same',lambda:False,2,limit)
        with self.assertRaises(ValueError):
            run_managed(staged,[],lambda:'same',lambda:True,2,1)
        self.assertFalse(self.pidfile.exists())

if __name__=='__main__': unittest.main()
