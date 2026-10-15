import os
from pathlib import Path
import signal
import subprocess
import sys
import unittest
from unittest.mock import patch
import launcher_signal
from test_ready_worker import ReadyTest
from ready_worker import run_managed


class SignalTest(unittest.TestCase):
    setUp = ReadyTest.setUp
    worker = ReadyTest.worker
    reaped = ReadyTest.reaped

    def test_real_signals_during_ready_reap_without_start(self):
        for sig in (signal.SIGINT, signal.SIGTERM):
            # Signal only this test process, after the synthetic child consumed input.
            staged = self.worker(f'os.kill(os.getppid(), {int(sig)})\ntime.sleep(10)\n')
            previous = {s: signal.getsignal(s) for s in (signal.SIGINT, signal.SIGTERM)}
            with launcher_signal.stop_latch() as stop:
                with self.assertRaisesRegex(ValueError, 'WORKER_STOPPED_BEFORE_START'):
                    run_managed(staged, [], lambda: 'same', stop, 2, 1)
                self.assertTrue(stop())
            self.assertEqual(previous, {s: signal.getsignal(s) for s in previous})
            self.reaped()

    def test_conflict_restoration_and_validation_stop(self):
        before = signal.getsignal(signal.SIGTERM)
        try:
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
            with self.assertRaisesRegex(ValueError, 'SIGNAL_CONFLICT'), launcher_signal.stop_latch():
                self.fail('yielded')
            self.assertEqual(signal.getsignal(signal.SIGTERM), signal.SIG_IGN)
        finally:
            signal.signal(signal.SIGTERM, before)
        def checked(*args, stop, **kwargs):
            self.assertFalse(stop())
            os.kill(os.getpid(), signal.SIGTERM)
            self.assertTrue(stop())
            raise ValueError('VALIDATION_STOP')
        with patch('staged_worker.checked_run', checked):
            with self.assertRaisesRegex(ValueError, 'VALIDATION_STOP'):
                launcher_signal.run()
        self.assertEqual(signal.getsignal(signal.SIGTERM), before)

    def test_subprocess_isolation(self):
        result = subprocess.run([sys.executable, '-m', 'unittest',
            'test_launcher_signal.SignalTest.test_real_signals_during_ready_reap_without_start',
            'test_launcher_signal.SignalTest.test_conflict_restoration_and_validation_stop'],
            cwd=Path(__file__).parent, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode())

if __name__ == '__main__': unittest.main()
