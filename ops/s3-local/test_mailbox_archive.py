import unittest
from unittest.mock import patch
import pid_mailbox
import test_prepared_session as fixtures
from prepared_session import PreparedSession

class ArchiveTest(unittest.TestCase):
    def fixture(self, fee=0):
        f = fixtures.PreparedSessionTest()
        self.addCleanup(f.doCleanups)
        return f.fixture(fee)

    def complete(self, f, p, kw):
        with patch('mailbox_session.release_inventory.probes', return_value=f.f.probes):
            with p.session(**kw):
                pid_mailbox.reporter(p.evidence_path)(2147483647)

    def test_two_sessions_same_packet_fresh_nonce_preserved_bytes(self):
        for fee in (0,25):
            f,p,kw = self.fixture(fee)
            self.complete(f,p,kw)
            before = {x.name:x.read_bytes() for x in p.evidence_path.iterdir()}
            saved = p.preserve_completed_mailbox()
            self.assertFalse(p.evidence_path.exists())
            self.assertEqual(before, {x.name:x.read_bytes() for x in saved.iterdir()})
            q = PreparedSession(*f.f.args, fee_bps=fee)
            self.assertEqual(p.packet, q.packet)
            self.assertNotEqual(p._mailbox.nonce, q._mailbox.nonce)
            with self.assertRaises(ValueError): p.preserve_completed_mailbox()
            self.assertTrue(q.evidence_path.exists())

    def test_not_started_and_interrupted_never_archive(self):
        for interrupt in (False,True):
            f,p,kw = self.fixture()
            if interrupt:
                with patch('mailbox_session.release_inventory.probes', return_value=f.f.probes):
                    with self.assertRaises(KeyboardInterrupt):
                        with p.session(**kw):
                            pid_mailbox.reporter(p.evidence_path)(2147483647)
                            raise KeyboardInterrupt()
            with self.assertRaises(ValueError): p.preserve_completed_mailbox()
            self.assertTrue((p.evidence_path/'challenge').exists())

    def test_changed_evidence_or_existing_destination_never_overwritten(self):
        for mode in ('report','extra','destination'):
            f,p,kw = self.fixture()
            self.complete(f,p,kw)
            if mode == 'report': (p.evidence_path/'pids.json').write_bytes(b'changed')
            if mode == 'extra': (p.evidence_path/'unexpected').write_bytes(b'evidence')
            if mode == 'destination':
                (p.evidence_path.parent/('completed-'+p._mailbox.nonce)).mkdir()
            with self.assertRaises(ValueError): p.preserve_completed_mailbox()
            self.assertTrue(p.evidence_path.exists())
            with self.assertRaises(ValueError): p.preserve_completed_mailbox()

    def test_fsync_failure_keeps_archive_and_consumes_attempt(self):
        f,p,kw = self.fixture()
        self.complete(f,p,kw)
        with patch('prepared_session.os.fsync', side_effect=OSError('fault')):
            with self.assertRaises(ValueError): p.preserve_completed_mailbox()
        saved = p.evidence_path.parent/('completed-'+p._mailbox.nonce)/'mailbox'
        self.assertTrue((saved/'pids.json').exists())
        with self.assertRaises(ValueError): p.preserve_completed_mailbox()

if __name__ == '__main__': unittest.main()
