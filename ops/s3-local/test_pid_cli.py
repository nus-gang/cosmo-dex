import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import managed_cli
import pid_mailbox
from test_managed_cli import ManagedCliTest as Fixture

class PidCliTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.box = pid_mailbox.Mailbox(Path(self.tmp.name).resolve() / 'pids')
        self.args = Fixture().args()
        self.args[self.args.index('--pid-mailbox')+1] = str(self.box.root)

    def test_actual_child_report_preserved_after_ready_cleanup(self):
        from test_ready_worker import ReadyTest
        from ready_worker import ready
        fixture = ReadyTest(); fixture.setUp(); self.addCleanup(fixture.doCleanups)
        staged = fixture.worker('s.sendall(b"READY\\n")\ns.recv(16)\n')
        def run(*args, **kwargs):
            self.assertNotIn('--pid-mailbox', args[9])
            with ready(staged, [], lambda: 'same', 2, on_spawn=kwargs['on_spawn']):
                self.assertTrue((self.box.root/'pids.json').exists())
            return {'worker_exit': 0, 'reusable_permit': False}
        with patch.object(managed_cli, 'run', side_effect=run) as invoked:
            self.assertEqual(Fixture().invoke(self.args), (0, '', ''))
            invoked.assert_called_once()
        pids = self.box.collect()
        self.assertEqual(pids, (os.getpid(), int(fixture.pidfile.read_text())))
        with self.assertRaises(ProcessLookupError): os.kill(pids[1], 0)
        self.assertTrue((self.box.root/'pids.json').exists())

    def test_required_private_mailbox_rejects_before_run(self):
        for value in ('relative', '/a/../b', str(self.box.root/'missing')):
            args = self.args.copy(); args[args.index('--pid-mailbox')+1] = value
            with patch.object(managed_cli, 'run') as run:
                self.assertEqual(Fixture().invoke(args), (2, '', 'LOCAL_MANAGED_WORKER_REJECTED\n'))
                run.assert_not_called()
        for args in (self.args[:-2], self.args+['--pid-mailbox', str(self.box.root)]):
            with patch.object(managed_cli, 'run') as run:
                self.assertEqual(Fixture().invoke(args)[0], 2)
                run.assert_not_called()

    def test_record_error_is_reported_and_not_erased(self):
        def run(*args, **kwargs):
            kwargs['on_spawn'](2147483647)
            raise KeyboardInterrupt()
        with patch.object(managed_cli, 'run', side_effect=run):
            self.assertEqual(Fixture().invoke(self.args), (2, '', 'LOCAL_MANAGED_WORKER_REJECTED\n'))
        self.assertEqual(self.box.collect(), (os.getpid(), 2147483647))
