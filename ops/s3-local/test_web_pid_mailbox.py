import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock
import test_managed_web
from web_pid_mailbox import WebMailbox, reporter


class WebPidTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.root.chmod(0o700)

    def test_subprocess_exact_pid_and_preserved_single_use_evidence(self):
        box = WebMailbox(self.root/'mailbox')
        p = subprocess.Popen([sys.executable, '-B', '-c',
            'from web_pid_mailbox import reporter; import sys; reporter(sys.argv[1])()',
            str(box.root)], cwd=Path(__file__).parent)
        self.assertEqual(p.wait(timeout=5), 0)
        self.assertEqual(box.collect(), (p.pid,))
        self.assertTrue((box.root/'pids.json').exists())
        with self.assertRaises(ValueError): box.collect()
        with self.assertRaises(ValueError): reporter(box.root)()

    def test_missing_nonce_changed_partial_and_worker_schema_rejected(self):
        for case in ('missing', 'nonce', 'partial', 'worker'):
            box = WebMailbox(self.root/case)
            if case == 'nonce':
                reporter(box.root)()
                (box.root/'challenge').write_text('0'*64)
            elif case in ('partial','worker'):
                p = box.root/'pids.json'
                p.write_text('{' if case == 'partial' else
                    '{"schema":"s3-local-pids/1","launcher_pid":100,"worker_pid":101}')
                p.chmod(0o600)
            with self.assertRaises(ValueError): box.collect()
            with self.assertRaises(ValueError): box.collect()
            self.assertTrue(box.root.exists())

    def test_callback_before_socket_and_failure_or_stop_prevents_socket(self):
        fixture = test_managed_web.ManagedWebTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        box = WebMailbox(self.root/'ok')
        record = reporter(box.root)
        def ready():
            self.assertEqual(fixture.factory.call_count, 0)
            record()
        fixture.run_web(on_ready=ready, clock=iter([0.,2.]).__next__)
        self.assertEqual(box.collect(), (os.getpid(),))
        for failure in (ValueError('write'), KeyboardInterrupt()):
            fixture.factory.reset_mock()
            with self.assertRaises(type(failure)):
                fixture.run_web(on_ready=Mock(side_effect=failure))
            fixture.factory.assert_not_called()
        fixture.factory.reset_mock()
        def stop_now(): fixture.stop.return_value = True
        with self.assertRaises(InterruptedError): fixture.run_web(on_ready=stop_now)
        fixture.factory.assert_not_called()

    def test_replaced_root_and_duplicate_callback_poison(self):
        box = WebMailbox(self.root/'replace')
        record = reporter(box.root)
        box.root.rename(self.root/'saved')
        WebMailbox(box.root)
        with self.assertRaises(ValueError): record()
        with self.assertRaises(ValueError): record()
        self.assertFalse((box.root/'pids.json').exists())

if __name__ == '__main__': unittest.main()
