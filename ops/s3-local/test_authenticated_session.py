from contextlib import contextmanager
import unittest
from unittest.mock import patch

import authenticated_session as target
from test_prepared_session import PreparedSessionTest as Fixtures
import pid_mailbox


class AuthenticatedSessionTest(unittest.TestCase):
    def fixture(self, fee=0):
        helper = Fixtures()
        self.addCleanup(helper.doCleanups)
        f, p, kw = helper.fixture(fee)
        self.events = []
        self.audit = patch.object(target.approval_gate, 'inspect', return_value={'fresh': True}).start()
        self.capture = patch.object(target.preflight, 'verify_input_set', return_value=(b'capture', {})).start()
        self.validate = patch.object(target.offline_check, 'validate_snapshot', return_value=('a'*64, {})).start()
        self.client = patch.object(target.RuntimeClient, 'from_environment', return_value=f.f.client).start()
        patch.object(target, 'broker_at', kw['broker']).start()
        patch('mailbox_session.release_inventory.probes', return_value=f.f.probes).start()
        self.addCleanup(patch.stopall)
        return f, p

    def test_current_run_wiring_and_observed_stop(self):
        for fee in (0, 25):
            f,p = self.fixture(fee)
            with target.session(p, f.f.client._workspace_id) as evidence:
                pid_mailbox.reporter(p.evidence_path)(2147483647)
            self.client.assert_called_once_with(f.f.client._workspace_id, 's3-worker-fee'+str(fee))
            self.assertEqual(f.f.events, ['start','stop','process','port','writer'])
            self.assertTrue(evidence['host_release_observations_complete'])
            self.assertFalse(evidence['cleanup_complete_verified'])
            self.assertEqual(self.audit.call_count, 3)
            self.assertEqual(self.capture.call_count, 3)
            self.assertTrue((p.evidence_path/'pids.json').is_file())
            with self.assertRaises(ValueError):
                with target.session(p, f.f.client._workspace_id): pass
            patch.stopall()

    def test_denial_or_capture_change_never_starts(self):
        for mode in ('denied', 'semantic', 'capture', 'revoke'):
            f,p = self.fixture()
            if mode == 'denied': self.audit.side_effect = ValueError('SECRET')
            if mode == 'semantic': self.validate.side_effect = ValueError('SECRET')
            if mode == 'capture': self.capture.side_effect = [(b'capture',{}),(b'changed',{})]
            if mode == 'revoke': self.audit.side_effect = [{'fresh':True},{'fresh':False}]
            with self.assertRaisesRegex(ValueError, '^AUTHENTICATED_SESSION_REJECTED$'):
                with target.session(p, f.f.client._workspace_id): pass
            f.f.client.request.assert_not_called()
            self.assertTrue((p.evidence_path/'challenge').is_file())
            with self.assertRaises(ValueError):
                with target.session(p, f.f.client._workspace_id): pass
            patch.stopall()

    def test_stop_before_or_during_audit_never_starts(self):
        for calls in (0, 1, 2):
            f,p = self.fixture()
            count = 0
            def stop():
                nonlocal count
                count += 1
                return count > calls
            with self.assertRaises(ValueError):
                with target.session(p, f.f.client._workspace_id, stop=stop): pass
            f.f.client.request.assert_not_called()
            patch.stopall()

    def test_last_audit_denial_before_control(self):
        f,p = self.fixture()
        self.audit.side_effect = [{'fresh':True},{'fresh':True},ValueError('SECRET')]
        with self.assertRaises(ValueError):
            with target.session(p, f.f.client._workspace_id): pass
        f.f.client.request.assert_not_called()

    def test_interrupt_stops_once_and_preserves_evidence(self):
        f,p = self.fixture()
        with self.assertRaises(KeyboardInterrupt):
            with target.session(p, f.f.client._workspace_id) as evidence:
                pid_mailbox.reporter(p.evidence_path)(2147483647)
                raise KeyboardInterrupt()
        self.assertEqual(f.f.events, ['start','stop','process','port','writer'])
        self.assertTrue((p.evidence_path/'pids.json').exists())
        self.assertFalse(p._retirable)


if __name__ == '__main__': unittest.main()
