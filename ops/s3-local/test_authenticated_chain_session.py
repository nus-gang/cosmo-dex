from contextlib import contextmanager
import hashlib
import os
import sys
import unittest
from unittest.mock import Mock, patch
import authenticated_chain_session as target
import test_chain_session as fixtures
import pid_mailbox as pid

class AuthenticatedChainTest(unittest.TestCase):
    def fixture(self, fee=0):
        f=fixtures.ChainSessionTest(); self.addCleanup(f.doCleanups); f.setup_case(fee)
        self.inspect=patch.object(target.approval_gate,'inspect',return_value={'fresh':True}).start()
        self.capture=patch.object(target.preflight,'verify_input_set',return_value=(b'capture',{})).start()
        patch.object(target.preflight,'checked_root',side_effect=lambda x:x).start()
        self.profile=patch.object(target.preflight,'bounded',return_value=b'profile').start()
        self.staged=Mock(capture_sha256=hashlib.sha256(b'capture').hexdigest(),
                         profile_sha256=hashlib.sha256(b'profile').hexdigest())
        @contextmanager
        def stage(*args):
            try: yield self.staged
            finally: f.events.append('stage-close')
        patch.object(target.chain_stage,'stage',stage).start()
        self.check=patch.object(target.chain_preflight,'check',return_value=dict(
            b_preflight=True,approval_verified=False,service_started=False,durable_ack=False)).start()
        self.client=patch.object(target.RuntimeClient,'from_environment',return_value=f.client).start()
        patch.object(target,'broker_at',f.kw['broker']).start()
        patch.object(target.chain_session.process_release,'check',f.kw['process']).start()
        patch.object(target.chain_session.port_release,'check',f.kw['port']).start()
        patch.object(target.chain_session.chain_writer_release,'check',f.kw['writer']).start()
        self.addCleanup(patch.stopall)
        return f

    def run_case(self,f,fee=0,stop=lambda:False):
        return target.session(sys.executable,'/candidate',f.argv,f.client._workspace_id,
            fee_bps=fee,validator_index=2,mailbox=f.mailbox,stop=stop)

    def test_profiles_preflight_and_scoped_cleanup(self):
        for fee in (0,25):
            f=self.fixture(fee)
            with self.run_case(f,fee) as evidence:
                pid.reporter(f.mailbox.root)(os.getpid()+1)
                self.check.assert_called_once()
                self.assertNotIn('stage-close',f.events)
            self.client.assert_called_once_with(f.client._workspace_id,f's3-chain-fee{fee}-v2')
            self.assertEqual(f.events,['start','stop','process','port','writer','broker-close','stage-close'])
            self.assertTrue(evidence['host_release_observations_complete'])
            self.assertFalse(evidence['cleanup_complete_verified'])
            self.assertTrue((f.mailbox.root/'pids.json').exists())
            with self.assertRaises(ValueError):
                with self.run_case(f,fee): pass
            patch.stopall()

    def test_changed_inputs_approval_stop_and_preflight_never_start(self):
        for mode in ('approval','semantic','capture','profile','digest','revoke','stop','last-audit','staged','report'):
            f=self.fixture()
            if mode=='approval': self.inspect.side_effect=OSError('secret')
            if mode=='semantic': self.check.side_effect=ValueError('secret')
            if mode=='capture': self.capture.side_effect=[(b'capture',{}),(b'changed',{})]
            if mode=='profile': self.profile.side_effect=[b'profile',b'changed']
            if mode=='digest': self.staged.profile_sha256='0'*64
            if mode=='revoke': self.inspect.side_effect=[{'fresh':True},{'fresh':False}]
            if mode=='last-audit': self.inspect.side_effect=[{'fresh':True},{'fresh':True},OSError('secret')]
            if mode=='staged': self.staged.verify.side_effect=ValueError('secret')
            if mode=='report': self.check.return_value={}
            with self.assertRaisesRegex(ValueError,'^'+target.ERROR+'$'):
                with self.run_case(f,stop=lambda:mode=='stop'): pass
            f.client.request.assert_not_called()
            self.assertTrue((f.mailbox.root/'challenge').exists())
            with self.assertRaises(ValueError):
                with self.run_case(f): pass
            patch.stopall()

    def test_interrupt_stops_once_preserves_evidence(self):
        f=self.fixture()
        with self.assertRaises(KeyboardInterrupt):
            with self.run_case(f):
                pid.reporter(f.mailbox.root)(os.getpid()+1)
                raise KeyboardInterrupt()
        self.assertEqual(f.events,['start','stop','process','port','writer','broker-close','stage-close'])
        self.assertTrue((f.mailbox.root/'pids.json').exists())

    def test_transport_eight_exact_selectors(self):
        from runtime_client import RuntimeClient
        from test_runtime_client import BASE,RUN,WID,LIST
        for fee in (0,25):
            for index in range(4):
                name=f's3-chain-fee{fee}-v{index}'
                c=RuntimeClient(BASE,'synthetic',RUN,WID,name); c._exchange=Mock(return_value={})
                path=LIST+f'/{WID}/runtime-services/start'
                c.request(dict(method='POST',path=path,body={'workspaceCommandId':name}))
                for other in ('all',f's3-chain-fee{fee}-v{(index+1)%4}','s3-web-fee0'):
                    with self.assertRaises(ValueError): c.request(dict(method='POST',path=path,body={'workspaceCommandId':other}))
                self.assertEqual(c._exchange.call_count,1)
        for bad in ('s3-chain-fee0-v4','s3-chain-fee25-v-1','s3-chain-fee01-v0'):
            with self.assertRaises(ValueError): RuntimeClient(BASE,'synthetic',RUN,WID,bad)

if __name__=='__main__': unittest.main()
