import json
import unittest
import storage_fault_run as target
import test_ready_worker as fixtures

class FaultRunTest(unittest.TestCase):
    setUp = fixtures.ReadyTest.setUp
    worker = fixtures.ReadyTest.worker
    reaped = fixtures.ReadyTest.reaped

    def child(self, after=''):
        return self.worker('assert sys.argv[1]=="fault-seal-captured"\n'
            's.sendall(b"READY\\n")\nassert s.recv(16)==b"START\\n"\nassert s.recv(1)==b""\n'
            f'open({str(self.root/"evidence")!r},"wb").write(b"preserve")\n'+after)

    def execute(self, staged, audit=lambda:'same', **kw):
        return target.run(staged, [], audit, point='before_wal', occurrence=1,
            purpose='NORMAL', evidence_root=self.root/'evidence', enable=True, timeout=2, **kw)

    def test_apply_child_command_result_and_final_revoke(self):
        obj=dict(schema='s3-local-fault-apply-result/1',command_succeeded=False,
                 injected=True,durable_ack=False,DEV='NOT_RUN')
        raw=(json.dumps(obj,sort_keys=True,separators=(',',':'))+'\n').encode()
        for revoke in (False,True):
            staged=self.worker('assert sys.argv[1]=="fault-apply-captured"\n'
                's.sendall(b"READY\\n")\nassert s.recv(16)==b"START\\n"\n'
                'assert s.recv(1)==b""\nsys.stdout.buffer.write('+repr(raw)+')\n')
            calls=[]
            def audit():
                calls.append(1)
                return 'changed' if revoke and len(calls)==3 else 'same'
            if revoke:
                with self.assertRaisesRegex(ValueError,'APPROVAL_CHANGED'):
                    self.execute(staged,audit,operation='Apply')
            else:
                self.assertEqual(self.execute(staged,audit,operation='Apply')['child_result'],obj)
            self.reaped()
        for operation in ('apply','Auto'):
            with self.assertRaisesRegex(ValueError,'STORAGE_FAULT_COMMAND'):
                self.execute(staged,operation=operation)

    def test_exact_report_distinguishes_command_failure(self):
        for succeeded in (False, True):
            obj=dict(schema='s3-local-fault-seal-result/1',command_succeeded=succeeded,
                     injected=True,durable_ack=False,DEV='NOT_RUN')
            raw=(json.dumps(obj,sort_keys=True,separators=(',',':'))+'\n').encode()
            staged=self.child('sys.stdout.buffer.write('+repr(raw)+')\n')
            calls=[]
            report=self.execute(staged,lambda:calls.append(1) or 'same')
            self.assertEqual(len(calls),3); self.assertEqual(report['child_result'],obj)
            self.assertFalse(report['replay_verified']); self.assertFalse(report['reusable_permit'])
            self.reaped(); self.assertEqual((self.root/'evidence').read_bytes(),b'preserve')

    def test_final_audit_denials_start_zero(self):
        for mode in ('revoke','binary','capture','stop','interrupt'):
            staged=self.child(); calls=[]; stopped=[False]
            def audit():
                calls.append(1)
                if len(calls)==3:
                    if mode=='revoke': return 'changed'
                    if mode=='binary':
                        staged.executable.chmod(0o700); staged.executable.write_bytes(b'changed')
                    if mode=='capture': object.__setattr__(staged,'capture',b'changed')
                    if mode=='stop': stopped[0]=True
                    if mode=='interrupt': raise KeyboardInterrupt()
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)):
                self.execute(staged,audit,stop=lambda:stopped[0])
            self.reaped(); self.assertFalse((self.root/'evidence').exists())

    def test_post_start_failure_preserves_unknown(self):
        for after in ('print("secret",flush=True)\n','sys.exit(2)\n','time.sleep(2)\n',
                      'sys.stderr.write("secret")\n'):
            staged=self.child(after)
            with self.assertRaisesRegex(ValueError,'OUTCOME_UNKNOWN'):
                self.execute(staged,lifetime=.3)
            self.reaped(); self.assertEqual((self.root/'evidence').read_bytes(),b'preserve')

    def test_limits_before_spawn_and_post_start_interrupt(self):
        staged=self.child()
        for value in (0,61,True,float('nan')):
            with self.assertRaisesRegex(ValueError,'LIFETIME_LIMIT'): self.execute(staged,lifetime=value)
        self.assertFalse(self.pidfile.exists())
        staged=self.child('time.sleep(2)\n')
        def stop():
            if (self.root/'evidence').exists(): raise KeyboardInterrupt()
            return False
        with self.assertRaises(KeyboardInterrupt): self.execute(staged,stop=stop)
        self.reaped(); self.assertEqual((self.root/'evidence').read_bytes(),b'preserve')
