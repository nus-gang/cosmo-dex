import contextlib
import os
import unittest
from unittest.mock import patch

import approval_gate as gate
import managed_cli
import private_reader
import test_approval_gate as fixtures
from test_native_review import uid
from test_review_documents import REVISIONS
from test_managed_cli import ManagedCliTest


class PrivateTransportTest(unittest.TestCase):
    def test_scope_restores_reader_on_error_without_fallback(self):
        with patch.object(gate.Reader, 'from_environment') as environment:
            with gate.private_transport('/one/s'):
                outer = gate._PRIVATE_READER.get()
                with self.assertRaises(KeyboardInterrupt):
                    with gate.private_transport('/two/s'):
                        self.assertNotEqual(gate._PRIVATE_READER.get(), outer)
                        raise KeyboardInterrupt()
                self.assertIs(gate._PRIVATE_READER.get(), outer)
                with patch.object(gate, '_inspect', side_effect=lambda reader, *a: reader(next(iter(private_reader.PATHS)))):
                    with self.assertRaisesRegex(ValueError, private_reader.ERROR):
                        gate.inspect(*([None]*7))
            self.assertIsNone(gate._PRIVATE_READER.get())
            environment.assert_not_called()

    def test_cli_scopes_socket_and_excludes_it_from_worker_arguments(self):
        args = ManagedCliTest().args()
        def run(*args, **kwargs):
            self.assertEqual(str(gate._PRIVATE_READER.get().endpoint), '/private/broker/s')
            self.assertNotIn('--approval-socket', args[9])
            raise ValueError('secret')
        with patch.object(managed_cli, 'reporter'), patch.object(managed_cli, 'run', side_effect=run) as invoked:
            self.assertEqual(ManagedCliTest().invoke(args), (2,'','LOCAL_MANAGED_WORKER_REJECTED\n'))
            invoked.assert_called_once()
        self.assertIsNone(gate._PRIVATE_READER.get())
        with patch.object(managed_cli, 'run') as run:
            for tail in ([], ['--approval-socket','relative'], ['--approval-socket','/a/../s'],
                         ['--approval-socket','/a/s','--approval-socket','/b/s']):
                self.assertEqual(ManagedCliTest().invoke(args[:-2]+tail)[0],2)
            run.assert_not_called()

    def test_real_ipc_composed_audit_and_revocation(self):
        fixture = fixtures.ApprovalGateTest()
        fixture.setUpClass()
        fixture.setUp()
        try:
            with private_reader._broker(os.environ['PAPERCLIP_RUN_SCRATCH_DIR'],fixture.reader) as endpoint:
                with gate.private_transport(endpoint), patch.object(gate.Reader,'from_environment') as environment:
                    def inspect():
                        return gate.inspect(fixture.bundle,fixture.artifacts,fixture.pin,
                            's3-dev-local/1',True,uid(5),REVISIONS)
                    self.assertTrue(inspect()['approval_prerequisites_match'])
                    self.assertEqual(len(fixture.calls),12)
                    fixture.issue['status']='in_progress'
                    with self.assertRaises(ValueError): inspect()
                    environment.assert_not_called()
            self.assertFalse(endpoint.parent.exists())
        finally:
            fixture.tearDown()
            fixture.doCleanups()

if __name__ == '__main__': unittest.main()
