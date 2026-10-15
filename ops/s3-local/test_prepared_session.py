import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import pid_mailbox
from prepared_session import PreparedSession
from test_mailbox_session import MailboxSessionTest


class PreparedSessionTest(unittest.TestCase):
    def fixture(self, fee=0, mode='ok'):
        f = MailboxSessionTest()
        f.setUp()
        self.addCleanup(f.doCleanups)
        f.setup_case(fee, mode)
        # Existing fixture mailbox is reserved for its own tests. Point both
        # registration and new owner at a fresh name instead.
        root = f.box.root.parent / 'prepared'
        argv = f.f.args[2]
        argv[argv.index('--pid-mailbox')+1] = str(root)
        p = PreparedSession(*f.f.args, fee_bps=fee)
        f.box = p._mailbox
        f.setup_case(fee, mode)
        inputs = dict(f.inputs)
        inputs.pop('mailbox')
        kwargs = dict(f.f.kwargs)
        kwargs.pop('fee_bps')
        return f, p, dict(inputs, **kwargs)

    def test_exact_packet_to_session_and_retention(self):
        for fee in (0,25):
            f,p,kw = self.fixture(fee)
            packet = p.packet
            self.assertIn(str(p.evidence_path), packet['body']['runtimeConfig']
                ['workspaceRuntime']['commands'][0]['command'])
            packet['body'].clear()
            self.assertTrue(p.packet['body'])
            with patch('mailbox_session.release_inventory.probes', return_value=f.f.probes):
                with p.session(**kw) as evidence:
                    pid_mailbox.reporter(p.evidence_path)(2147483647)
            self.assertEqual(f.f.events, ['start','stop','process','port','writer'])
            self.assertTrue(evidence['pid_handoff_collected'])
            self.assertFalse(evidence['cleanup_complete_verified'])
            self.assertTrue((p.evidence_path/'pids.json').is_file())
            with self.assertRaises(ValueError):
                with p.session(**kw): pass

    def test_duplicate_creation_and_invalid_arguments_preserve_evidence(self):
        f,p,kw = self.fixture()
        challenge = (p.evidence_path/'challenge').read_bytes()
        with self.assertRaises(ValueError): PreparedSession(*f.f.args, fee_bps=0)
        self.assertEqual((p.evidence_path/'challenge').read_bytes(), challenge)
        argv = list(f.f.args[2])
        bad = p.evidence_path.parent/'invalid'
        argv[argv.index('--pid-mailbox')+1] = str(bad)
        argv += ['--unknown']
        with self.assertRaises(ValueError):
            PreparedSession(*f.f.args[:2], argv, fee_bps=0)
        self.assertFalse(bad.exists())

    def test_denial_and_interrupt_consume_owner_without_erasing_evidence(self):
        for kind in ('deny','interrupt'):
            f,p,kw = self.fixture()
            error = ValueError('denied') if kind == 'deny' else KeyboardInterrupt()
            with patch('prepared_session.mailbox_session.session', side_effect=error):
                with self.assertRaises(type(error)):
                    with p.session(**kw): pass
            with self.assertRaises(ValueError):
                with p.session(**kw): pass
            self.assertTrue((p.evidence_path/'challenge').is_file())
            f.f.client.request.assert_not_called()

    def test_overrides_refused_before_control(self):
        for name in ('mailbox','fee_bps'):
            f,p,kw = self.fixture()
            kw[name] = None
            with self.assertRaisesRegex(ValueError, 'PREPARED_SESSION_REJECTED'):
                with p.session(**kw): pass
            f.f.client.request.assert_not_called()
            self.assertTrue((p.evidence_path/'challenge').is_file())

if __name__ == '__main__': unittest.main()
