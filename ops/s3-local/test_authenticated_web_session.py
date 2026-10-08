import hashlib
from types import SimpleNamespace
import sys
import unittest
from unittest.mock import patch
import authenticated_web_session as target
import test_web_session as fixtures
import web_pid_mailbox as pid

class AuthenticatedWebTest(unittest.TestCase):
    def fixture(self, fee=0):
        f=fixtures.WebSessionTest(); self.addCleanup(f.doCleanups); f.setup_case(fee)
        self.inspect=patch.object(target.approval_gate,'inspect',return_value={'fresh':True}).start()
        self.capture=patch.object(target.preflight,'verify_input_set',return_value=(b'capture',{})).start()
        self.prepare=patch.object(target.reviewed_web,'prepare',return_value=SimpleNamespace(
            capture_sha256=hashlib.sha256(b'capture').hexdigest())).start()
        self.client=patch.object(target.RuntimeClient,'from_environment',return_value=f.client).start()
        patch.object(target,'broker_at',f.kw['broker']).start()
        patch.object(target.web_session.process_release,'check',f.kw['process']).start()
        patch.object(target.web_session.port_release,'check',f.kw['port']).start()
        self.addCleanup(patch.stopall)
        return f

    def run_case(self,f,fee=0,stop=lambda:False):
        return target.session(sys.executable,'/candidate',f.argv,f.client._workspace_id,
            fee_bps=fee,mailbox=f.mailbox,stop=stop)

    def test_profiles_current_auth_and_stop_evidence(self):
        for fee in (0,25):
            f=self.fixture(fee)
            with self.run_case(f,fee) as evidence: pid.reporter(f.mailbox.root)()
            self.client.assert_called_once_with(f.client._workspace_id,'s3-web-fee'+str(fee))
            self.assertEqual(f.events,['start','stop','process','port','broker-close'])
            self.assertTrue(evidence['host_release_observations_complete'])
            self.assertFalse(evidence['cleanup_complete_verified'])
            self.assertTrue((f.mailbox.root/'pids.json').exists())
            with self.assertRaises(ValueError):
                with self.run_case(f,fee): pass
            patch.stopall()

    def test_denials_changes_and_stop_never_start(self):
        for mode in ('approval','semantic','capture','digest','revoke','stop','last-audit'):
            f=self.fixture()
            if mode=='approval': self.inspect.side_effect=OSError('secret')
            if mode=='semantic': self.prepare.side_effect=ValueError('secret')
            if mode=='capture': self.capture.side_effect=[(b'capture',{}),(b'changed',{})]
            if mode=='digest': self.prepare.return_value.capture_sha256='0'*64
            if mode=='revoke': self.inspect.side_effect=[{'fresh':True},{'fresh':False}]
            if mode=='last-audit': self.inspect.side_effect=[{'fresh':True},{'fresh':True},OSError('secret')]
            with self.assertRaisesRegex(ValueError,'^'+target.ERROR+'$'):
                with self.run_case(f,stop=lambda:mode=='stop'): pass
            f.client.request.assert_not_called()
            self.assertTrue((f.mailbox.root/'challenge').exists())
            with self.assertRaises(ValueError):
                with self.run_case(f): pass
            patch.stopall()

    def test_interrupt_stops_once_preserves_pid(self):
        f=self.fixture()
        with self.assertRaises(KeyboardInterrupt):
            with self.run_case(f):
                pid.reporter(f.mailbox.root)(); raise KeyboardInterrupt()
        self.assertEqual(f.events,['start','stop','process','port','broker-close'])
        self.assertTrue((f.mailbox.root/'pids.json').exists())

    def test_transport_web_selector_remains_exact(self):
        from runtime_client import RuntimeClient
        from test_runtime_client import BASE,RUN,WID,LIST
        from unittest.mock import Mock
        for fee in (0,25):
            c=RuntimeClient(BASE,'synthetic',RUN,WID,'s3-web-fee'+str(fee)); c._exchange=Mock(return_value={})
            path=LIST+f'/{WID}/runtime-services/start'
            c.request(dict(method='POST',path=path,body={'workspaceCommandId':'s3-web-fee'+str(fee)}))
            c._exchange.assert_called_once()
            for name in ('s3-worker-fee'+str(fee),'s3-web-fee'+str(25-fee),'all'):
                with self.assertRaises(ValueError): c.request(dict(method='POST',path=path,body={'workspaceCommandId':name}))
            self.assertEqual(c._exchange.call_count,1)

if __name__=='__main__': unittest.main()
