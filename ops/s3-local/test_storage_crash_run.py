import json
import unittest
import storage_crash_run as target
import test_ready_worker as fixtures

class CrashRunTest(unittest.TestCase):
    setUp = fixtures.ReadyTest.setUp
    worker = fixtures.ReadyTest.worker
    reaped = fixtures.ReadyTest.reaped

    def child(self, after=''):
        return self.worker('assert sys.argv[1]=="crash-seal-captured"\n'
            'assert "--enable-storage-crash" in sys.argv\n'
            'assert "--worker-inputs" in sys.argv\n'
            's.sendall(b"READY\\n")\nassert s.recv(16)==b"START\\n"\nassert s.recv(1)==b""\n'
            f'open({str(self.root/"evidence")!r},"wb").write(b"preserve")\n'+after)

    def execute(self, staged, audit=lambda:'same', **kw):
        return target.run(staged, [], audit, point='before_wal', occurrence=1,
            purpose='NORMAL', evidence_root=self.root/'evidence', enable=True, timeout=2, **kw)

    def test_exit86_is_unknown_not_crash_proof(self):
        calls=[]
        result=self.execute(self.child('os._exit(86)\n'),lambda:calls.append(1) or 'same')
        self.assertEqual(len(calls),3)
        self.assertEqual(result['child_exit'],86)
        self.assertEqual(result['outcome'],'UNKNOWN')
        self.assertIsNone(result['child_result'])
        for key in ('crash_verified','replay_verified','approval_verified','reusable_permit'):
            self.assertIs(result[key],False)
        self.reaped()
        self.assertEqual((self.root/'evidence').read_bytes(),b'preserve')

    def test_not_reached_does_not_imply_command_success(self):
        for ok in (False,True):
            obj=dict(schema='s3-local-crash-seal-result/1',command_succeeded=ok,
                     crash_reached=False,crash_verified=False,durable_ack=False,DEV='NOT_RUN')
            raw=(json.dumps(obj,sort_keys=True,separators=(',',':'))+'\n').encode()
            result=self.execute(self.child('sys.stdout.buffer.write('+repr(raw)+')\n'))
            self.assertEqual(result['child_result'],obj)
            self.assertEqual(result['outcome'],'RECORDED_NOT_REACHED')
            self.reaped()

    def test_final_gate_denials_start_zero(self):
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

    def test_failure_limits_and_interrupt_preserve_evidence(self):
        for after in ('print("secret",flush=True)\n','sys.exit(2)\n','time.sleep(2)\n',
                      'sys.stderr.write("secret")\n','sys.stdout.write("{"); sys.stdout.flush(); os._exit(86)\n'):
            with self.assertRaisesRegex(ValueError,'OUTCOME_UNKNOWN'):
                self.execute(self.child(after),lifetime=.3)
            self.reaped(); self.assertEqual((self.root/'evidence').read_bytes(),b'preserve')
        staged=self.child()
        for value in (0,61,True,float('nan')):
            with self.assertRaisesRegex(ValueError,'LIFETIME_LIMIT'): self.execute(staged,lifetime=value)
        (self.root/'evidence').unlink()
        def stop():
            if (self.root/'evidence').exists(): raise KeyboardInterrupt()
            return False
        with self.assertRaises(KeyboardInterrupt): self.execute(self.child('time.sleep(2)\n'),stop=stop)
        self.reaped(); self.assertEqual((self.root/'evidence').read_bytes(),b'preserve')
