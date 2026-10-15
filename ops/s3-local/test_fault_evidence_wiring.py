import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import managed_web
import test_managed_web
import test_web_cli
import web_cli


class EvidenceWiringTest(unittest.TestCase):
    def setUp(self):
        self.fixture = test_managed_web.ManagedWebTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.path = self.root / 'response-loss.jsonl'

    def run_web(self):
        return self.fixture.run_web(response_loss_sha256='a'*64, fault_evidence_root=self.root)

    def test_reservation_precedes_pid_socket_and_final_is_preserved(self):
        def socket(*args):
            rows = self.path.read_text().splitlines()
            self.assertEqual(len(rows), 1)
            self.assertEqual(json.loads(rows[0])['phase'], 'reserved')
            return self.fixture.listener
        self.fixture.factory.side_effect = socket
        result = self.fixture.run_web(response_loss_sha256='a'*64, fault_evidence_root=self.root,
                                     clock=iter([0.0, 2.0]).__next__)
        self.assertTrue(result['listener_closed'])
        self.assertEqual(json.loads(self.path.read_text().splitlines()[1])['phase'], 'returned')
        self.fixture.factory.reset_mock()
        with self.assertRaises(FileExistsError): self.run_web()
        self.fixture.factory.assert_not_called()

    def test_fsync_failure_blocks_socket_and_retains_evidence(self):
        with patch('fault_evidence.os.fsync', side_effect=OSError('sync')):
            with self.assertRaises(OSError): self.run_web()
        self.fixture.factory.assert_not_called()
        self.assertTrue(self.path.exists())

    def test_bind_error_and_interrupt_finalize_and_close(self):
        for error in (OSError('secret'), KeyboardInterrupt()):
            self.fixture.listener.bind.side_effect = error
            self.fixture.listener.reset_mock()
            with self.assertRaises(type(error)): self.run_web()
            self.fixture.listener.close.assert_called_once()
            rows = [json.loads(x) for x in self.path.read_text().splitlines()]
            self.assertEqual(rows[-1]['phase'], 'interrupted_or_failed')
            self.assertFalse(rows[-1]['report']['upstream_attempted'])
            self.assertNotIn('secret', self.path.read_text())
            self.path.unlink()

    def test_cli_requires_pair_and_keeps_evidence_out_of_validator_argv(self):
        args = test_web_cli.WebCliTest.args(self)
        fault = ['--drop-broadcast-response-sha256', 'a'*64]
        evidence = ['--fault-evidence-root', str(self.root)]
        parsed = web_cli.parse_web(args + fault + evidence)
        self.assertEqual(parsed[0].fault_evidence_root, self.root)
        self.assertNotIn('--fault-evidence-root', parsed[1])
        for extra in (fault, evidence, fault+evidence+evidence,
                      fault+['--fault-evidence-root', 'relative']):
            with self.assertRaises(ValueError): web_cli.parse_web(args+extra)
