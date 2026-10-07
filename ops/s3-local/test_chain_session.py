import copy
from contextlib import contextmanager
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock

import chain_session as subject
import pid_mailbox as pid
import test_chain_cli as fixture
import test_runtime_evidence as ev
from native_review import COMPANY
from workspace_registration import PROJECT, prepare_chain

class ChainSessionTest(unittest.TestCase):
    def setup_case(self, fee=0, mode='ok'):
        tmp = tempfile.TemporaryDirectory(); self.addCleanup(tmp.cleanup)
        root = Path(tmp.name).resolve(); root.chmod(0o700)
        self.mailbox = pid.Mailbox(root/'pids')
        self.argv = fixture.ChainCliTest.args(self)[1:]
        self.argv[self.argv.index('--pid-mailbox')+1] = str(self.mailbox.root)
        self.argv[self.argv.index('--approval-socket')+1] = '/private/broker/s'
        packet = prepare_chain(sys.executable, '/candidate', self.argv, fee_bps=fee, validator_index=2)
        body = packet['body']; config = body['runtimeConfig']; command = config['workspaceRuntime']['commands'][0]
        before = dict(copy.deepcopy(body), id=ev.WID, companyId=COMPANY, projectId=PROJECT, runtimeServices=[])
        after = copy.deepcopy(before)
        after['runtimeServices'] = [dict(id=ev.SID, companyId=COMPANY, projectId=PROJECT,
            projectWorkspaceId=ev.WID, provider='local_process', status='stopped',
            serviceName=command['name'], command=command['command'], cwd=command['cwd'],
            port=command['port'], url=None, exposure=None, startedAt=ev.START, stoppedAt=ev.END)]
        self.events=[]; self.client=Mock(); self.client._workspace_id=ev.WID
        self.client.read_workspaces.side_effect=[[copy.deepcopy(before)], [copy.deepcopy(before)], [after]]
        def request(packet):
            action=packet['path'].rsplit('/',1)[1]; self.events.append(action)
            self.assertEqual(packet['body'], {'workspaceCommandId':f's3-chain-fee{fee}-v2'})
            if mode==action+'-io': raise OSError('secret')
            return dict(workspace=copy.deepcopy(after), operation=dict(id=ev.OID, companyId=COMPANY,
                status='succeeded', command=command['command'], cwd=command['cwd'],
                startedAt=ev.START, finishedAt=ev.END,
                phase='workspace_teardown' if action=='stop' else 'workspace_provision',
                metadata=dict(action=action,projectId=PROJECT,projectWorkspaceId=ev.WID,
                    workspaceCommandId=command['id'],workspaceCommandKind='service')))
        self.client.request.side_effect=request
        @contextmanager
        def broker(root):
            try: yield root/'s'
            finally: self.events.append('broker-close')
        def process(pids):
            self.events.append('process')
            return dict(schema='s3-local-process-probe/1',pids=list(pids),listed_pids_absent=True)
        def port(endpoints):
            self.events.append('port'); self.assertEqual(endpoints,(('127.0.0.1',26657),('127.0.0.1',26656)))
            return dict(schema='s3-local-port-probe/1',endpoints=list(endpoints),simultaneous_bind_verified=True)
        def writer(home):
            self.events.append('writer')
            return dict(schema='s3-local-chain-writer-probe/1',home=str(home),
                writer_lock_reacquired=True,continuous_exclusion_verified=False)
        self.kw=dict(writer=Mock(side_effect=writer),fee_bps=fee,validator_index=2,mailbox=self.mailbox,broker_root='/private/broker',broker=broker,
            client=self.client,audit=Mock(),process=Mock(side_effect=process),port=Mock(side_effect=port))

    def run_case(self, failure=None, record=True):
        with subject._session(sys.executable,'/candidate',self.argv,**self.kw) as result:
            self.result=result
            if record: pid.reporter(self.mailbox.root)(os.getpid()+1)
            if failure: raise failure
        return result

    def test_exact_fee_profiles_and_retained_one_shot_evidence(self):
        for fee in (0,25):
            self.setup_case(fee); r=self.run_case()
            self.assertEqual(self.events,['start','stop','process','port','writer','broker-close'])
            self.assertEqual(r['listed_pids'],[os.getpid(),os.getpid()+1])
            self.assertTrue(r['host_release_observations_complete'])
            self.assertEqual(set(r['host_release_observations']),{'process','port','writer'})
            for k in ('inventory_complete_verified','cleanup_complete_verified','approval_verified'):
                self.assertFalse(r[k])
            self.assertTrue((self.mailbox.root/'pids.json').is_file())
            with self.assertRaises(ValueError): self.run_case()
            self.assertEqual(self.client.request.call_count,2)

    def test_writer_failure_preserves_prior_observations(self):
        for mode in ('error', 'report', 'interrupt'):
            self.setup_case()
            self.kw['writer'].side_effect = (KeyboardInterrupt() if mode == 'interrupt' else OSError('secret')) if mode != 'report' else None
            self.kw['writer'].return_value = {}
            with self.assertRaises(KeyboardInterrupt if mode == 'interrupt' else ValueError): self.run_case()
            self.assertEqual(set(self.result['host_release_observations']), {'process','port'})
            self.assertFalse(self.result['host_release_observations_complete'])
            self.kw['writer'].assert_called_once()
            self.assertTrue((self.mailbox.root/'pids.json').exists())

    def test_denial_config_drift_or_wrong_mailbox_starts_nothing(self):
        for mode in ('denial','drift','mailbox'):
            self.setup_case()
            if mode=='denial': self.kw['audit'].side_effect=OSError('secret')
            if mode=='drift':
                original = self.client.read_workspaces.side_effect
                first = next(original); changed = copy.deepcopy(first)
                changed[0]['runtimeConfig']['desiredState']='running'
                self.client.read_workspaces.side_effect=[first,changed]
            if mode=='mailbox': self.argv[self.argv.index('--pid-mailbox')+1]='/wrong'
            with self.assertRaises(ValueError): self.run_case()
            self.client.request.assert_not_called()
            self.assertFalse(self.mailbox.used)

    def test_ambiguous_start_and_stop_failure_no_host_probes(self):
        for mode in ('start-io','stop-io'):
            self.setup_case(mode=mode)
            with self.assertRaises(ValueError): self.run_case()
            self.assertEqual(self.events,['start','stop','broker-close'])
            self.kw['process'].assert_not_called(); self.kw['port'].assert_not_called()
            self.assertFalse(self.mailbox.used)

    def test_missing_or_mismatched_report_preserves_partial_evidence(self):
        for mode in ('missing','process','port'):
            self.setup_case()
            if mode!='missing':
                self.kw[mode].side_effect=None; self.kw[mode].return_value={}
            with self.assertRaisesRegex(ValueError,subject.RELEASE_ERROR): self.run_case(record=mode!='missing')
            self.assertFalse(self.result['host_release_observations_complete'])
            self.assertEqual(len(self.result['host_release_observations']),1 if mode=='port' else 0)
            self.assertTrue(self.mailbox.used)
            if mode!='port': self.kw['port'].assert_not_called()

    def test_body_and_probe_interrupt_do_not_retry_or_erase(self):
        for mode in ('body','probe'):
            self.setup_case()
            if mode=='probe': self.kw['process'].side_effect=KeyboardInterrupt()
            with self.assertRaises(KeyboardInterrupt): self.run_case(KeyboardInterrupt() if mode=='body' else None)
            self.assertEqual(self.client.request.call_count,2)
            self.kw['process'].assert_called_once()
            self.assertTrue((self.mailbox.root/'pids.json').is_file())
            self.assertEqual(self.events[-1],'broker-close')

if __name__=='__main__': unittest.main()
