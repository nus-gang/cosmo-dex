import socket
import unittest
from unittest.mock import patch, Mock
import managed_web
import test_reviewed_web


class ManagedWebTest(unittest.TestCase):
    def setUp(self):
        fixture = test_reviewed_web.ReviewedWebTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.prepared = fixture.prepare()
        self.events = []
        self.listener = Mock(family=socket.AF_INET, type=socket.SOCK_STREAM)
        self.listener.getsockname.return_value = ('127.0.0.1', 5173)
        self.listener.bind.side_effect = lambda addr: self.events.append(('bind', addr))
        self.listener.listen.side_effect = lambda n: self.events.append(('listen', n))
        self.listener.close.side_effect = lambda: self.events.append('close')
        self.factory = Mock(return_value=self.listener)
        self.audit = fixture.audit
        self.audit.reset_mock()
        self.audit.side_effect = lambda *a: self.events.append('audit') or {'same': True}
        self.prepare = patch.object(managed_web.reviewed_web, 'prepare',
            side_effect=lambda *a: self.events.append('prepare') or self.prepared).start()
        self.addCleanup(patch.stopall)
        self.stop = Mock(return_value=False)

    def run_web(self, **kwargs):
        return managed_web.run('bundle','artifacts','pin','s3-dev-local/1',True,
            'decision',{},'inputs','input.json',[],'scratch','http://127.0.0.1:5173',
            8787, max_requests=1, lifetime_seconds=1, stop=self.stop,
            socket_factory=self.factory, **kwargs)

    def test_fresh_audit_bind_order_and_bounded_listener_close(self):
        # End by lifetime before accept: real lifecycle, fake clock/socket only.
        result = self.run_web(clock=iter([0.0, 2.0]).__next__)
        self.assertEqual(self.events, ['audit','prepare','audit',
            ('bind',('127.0.0.1',5173)),('listen',1),'close'])
        self.listener.accept.assert_not_called()
        self.assertFalse(result['approval_verified'])
        self.assertFalse(result['reusable_permit'])
        self.assertEqual(result['capture_sha256'], self.prepared.capture_sha256)
        self.listener.close.assert_called_once()

    def test_denial_revocation_semantic_failure_and_stop_create_no_socket(self):
        for stage in ('initial','revoked','semantic','stop'):
            with self.subTest(stage=stage):
                self.audit.side_effect = [ValueError('denied')] if stage=='initial' else (
                    [{'same':True},{'same':False}] if stage=='revoked' else None)
                self.prepare.side_effect = ValueError('semantic') if stage=='semantic' else None
                self.prepare.return_value = self.prepared
                self.stop.return_value = stage=='stop'
                with self.assertRaises((ValueError, InterruptedError)): self.run_web()
                self.factory.assert_not_called()

    def test_bind_listen_failure_and_interrupt_close_once(self):
        for method, failure in [('bind',OSError('busy')),('listen',OSError('io')),
                                ('bind',KeyboardInterrupt())]:
            with self.subTest(method=method):
                self.listener.reset_mock()
                self.listener.bind.side_effect = None
                self.listener.listen.side_effect = None
                getattr(self.listener,method).side_effect = failure
                with self.assertRaises(type(failure)): self.run_web()
                self.listener.close.assert_called_once()
                self.listener.accept.assert_not_called()

    def test_stop_after_socket_creation_closes_without_bind(self):
        self.stop.side_effect = [False,False,False,False,True]
        with self.assertRaises(InterruptedError): self.run_web()
        self.listener.bind.assert_not_called()
        self.listener.close.assert_called_once()

if __name__ == '__main__': unittest.main()
