import unittest

import before_send_run as target
import test_ready_worker as fixtures


class BeforeSendRunTest(unittest.TestCase):
    setUp = fixtures.ReadyTest.setUp
    worker = fixtures.ReadyTest.worker
    reaped = fixtures.ReadyTest.reaped

    def child(self, after=''):
        return self.worker(
            'assert sys.argv[1]=="f05-crash-captured"\n'
            's.sendall(b"READY\\n")\n'
            'assert s.recv(16)==b"START\\n"\n'
            'assert s.recv(1)==b""\n'
            f'open({str(self.root / "evidence")!r},"wb").write(b"preserve")\n' + after
        )

    def execute(self, staged, audit=lambda: 'same', **kwargs):
        return target.run(staged, [], audit, tx_hash='a' * 64,
                          evidence_root=self.root / 'evidence', enable=True,
                          timeout=2, **kwargs)

    def test_exit86_is_unknown_and_never_overclaimed(self):
        staged = self.child('os._exit(86)\n')
        calls = []
        report = self.execute(staged, lambda: calls.append(1) or 'same')
        self.assertEqual(calls, [1, 1, 1])
        self.assertEqual(report, {
            'child_result': None, 'child_exit': 86, 'fault_started': True,
            'outcome': 'UNKNOWN', 'transport_called_verified': False,
            'crash_verified': False, 'approval_verified': False,
            'reusable_permit': False, 'replay_verified': False,
            'F05_verified': False,
        })
        self.reaped()
        self.assertEqual((self.root / 'evidence').read_bytes(), b'preserve')

    def test_final_gate_denials_send_no_start(self):
        for mode in ('revoke', 'binary', 'capture', 'stop', 'interrupt'):
            staged = self.child()
            calls, stopped = [], [False]

            def audit():
                calls.append(1)
                if len(calls) == 3:
                    if mode == 'revoke':
                        return 'changed'
                    if mode == 'binary':
                        staged.executable.chmod(0o700)
                        staged.executable.write_bytes(b'changed')
                    if mode == 'capture':
                        object.__setattr__(staged, 'capture', b'changed')
                    if mode == 'stop':
                        stopped[0] = True
                    if mode == 'interrupt':
                        raise KeyboardInterrupt()
                return 'same'

            with self.assertRaises((ValueError, KeyboardInterrupt)):
                self.execute(staged, audit, stop=lambda: stopped[0])
            self.reaped()
            self.assertFalse((self.root / 'evidence').exists())

    def test_post_start_output_exit_and_timeout_remain_unknown(self):
        for after in ('print("secret",flush=True)\n', 'sys.exit(2)\n',
                      'time.sleep(2)\n', 'sys.stderr.write("secret")\n'):
            staged = self.child(after)
            with self.assertRaisesRegex(ValueError, 'OUTCOME_UNKNOWN'):
                self.execute(staged, lifetime=.3)
            self.reaped()
            self.assertEqual((self.root / 'evidence').read_bytes(), b'preserve')
            (self.root / 'evidence').unlink()

    def test_limits_precede_spawn_and_interrupt_preserves_unknown(self):
        staged = self.child()
        for value in (0, 61, True, float('nan')):
            with self.assertRaisesRegex(ValueError, 'LIFETIME_LIMIT'):
                self.execute(staged, lifetime=value)
        self.assertFalse(self.pidfile.exists())
        staged = self.child('time.sleep(2)\n')

        def stop():
            if (self.root / 'evidence').exists():
                raise KeyboardInterrupt()
            return False

        with self.assertRaises(KeyboardInterrupt):
            self.execute(staged, stop=stop)
        self.reaped()
        self.assertEqual((self.root / 'evidence').read_bytes(), b'preserve')
