import unittest
from unittest.mock import patch
import storage_fault_ready as target
import test_ready_worker as fixtures

class FaultReadyTest(unittest.TestCase):
    setUp = fixtures.ReadyTest.setUp
    worker = fixtures.ReadyTest.worker
    reaped = fixtures.ReadyTest.reaped

    def probe(self, staged, audit=lambda: 'same', **kw):
        options = dict(point='before_wal', occurrence=1, purpose='NORMAL',
                       evidence_root=self.root/'evidence', enable=True, timeout=2)
        options.update(kw)
        return target.ready(staged, ['--worker-input', 'exact'], audit, **options)

    def test_exact_fault_argv_ready_no_start_reap(self):
        expected = target.fault_arguments('before_wal', 1, 'NORMAL', self.root/'evidence', True)
        staged = self.worker('assert sys.argv[1]=="fault-seal-captured"\n'
            'assert sys.argv[4]=="--capture-sha256"\n'
            'assert sys.argv[6:]=='+repr(expected+['--worker-input','exact'])+'\n'
            's.sendall(b"READY\\n")\n'
            f'if s.recv(16): open({str(self.root/"started")!r},"w").write("BAD")\n')
        calls=[]
        with self.probe(staged,lambda: calls.append(1) or 'same') as report:
            self.assertFalse(report['fault_started']); self.assertFalse(report['reusable_permit'])
            self.assertEqual(len(calls),2)
        self.reaped()
        self.assertFalse((self.root/'started').exists())
        self.assertFalse((self.root/'evidence').exists())

    def test_revoke_mutation_stop_interrupt_close(self):
        for mode in ('revoke','mutation','stop','interrupt'):
            staged=self.worker('s.sendall(b"READY\\n")\ns.recv(16)\n')
            calls=[]; stopped=[False]
            def audit():
                calls.append(1)
                if len(calls)==2:
                    if mode=='revoke': return 'different'
                    if mode=='mutation':
                        staged.executable.chmod(0o700); staged.executable.write_bytes(b'changed')
                    if mode=='stop': stopped[0]=True
                    if mode=='interrupt': raise KeyboardInterrupt()
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)), self.probe(staged,audit,stop=lambda:stopped[0]):
                self.fail('yielded')
            self.reaped(); self.assertFalse((self.root/'evidence').exists())

    def test_invalid_selection_spawn_zero(self):
        staged=self.worker('time.sleep(2)\n')
        for change in ({'enable':False},{'point':'unknown'},{'occurrence':True},
                       {'occurrence':0},{'occurrence':1025},{'purpose':'AUTO'},
                       {'evidence_root':'relative'},{'evidence_root':'/a/../b'}):
            with self.assertRaises(ValueError), self.probe(staged,**change): self.fail('yielded')
        with patch.object(target,'_ready_scope') as scope:
            with self.assertRaisesRegex(ValueError,'DUPLICATE_FAULT_OPTIONS'), target.ready(
                staged,['--fault-point','before_wal'],lambda:'same',point='before_wal',
                occurrence=1,purpose='NORMAL',evidence_root=self.root,enable=True): self.fail('yielded')
            scope.assert_not_called()
        self.assertFalse(self.pidfile.exists())
