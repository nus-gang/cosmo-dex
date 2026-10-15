import copy
import sys
import unittest
from unittest.mock import Mock

import managed_session
import runtime_evidence as ev
from native_review import COMPANY
from workspace_registration import PROJECT
import test_managed_session as fixture

WID = '11111111-1111-4111-8111-111111111111'
OID = '22222222-2222-4222-8222-222222222222'
SID = '33333333-3333-4333-8333-333333333333'
START = '2026-10-07T04:00:00.000Z'
END = '2026-10-07T04:01:00.000Z'


class EvidenceTest(unittest.TestCase):
    def setup_case(self, fee=0):
        self.case = fixture.SessionTest(); self.case.setup_case(fee)
        self.config = copy.deepcopy(self.case.workspace['runtimeConfig'])
        self.command = self.config['workspaceRuntime']['commands'][0]
        self.service = dict(id=SID, companyId=COMPANY, projectId=PROJECT,
            projectWorkspaceId=WID, provider='local_process', status='stopped',
            serviceName=self.command['name'], command=self.command['command'],
            cwd=self.command['cwd'], port=self.command['port'], url=None,
            exposure=None, startedAt=START, stoppedAt=END)
        self.workspace = copy.deepcopy(self.case.workspace)
        self.workspace['runtimeServices'] = [self.service]

    def response(self, action='stop'):
        return dict(workspace=copy.deepcopy(self.workspace), operation=dict(id=OID,
            companyId=COMPANY, status='succeeded', command=self.command['command'],
            cwd=self.command['cwd'], startedAt=START, finishedAt=END,
            phase='workspace_teardown' if action=='stop' else 'workspace_provision',
            metadata=dict(action=action, projectId=PROJECT, projectWorkspaceId=WID,
                          workspaceCommandId=self.command['id'], workspaceCommandKind='service')))

    def test_stop_evidence_fee_profiles_does_not_claim_host_exit(self):
        for fee in (0,25):
            self.setup_case(fee)
            result = ev.stopped(self.response(), [self.workspace], WID, self.config)
            self.assertTrue(result['control_plane_stop_verified'])
            for key in ('process_exit_verified','port_release_verified','writer_release_verified'):
                self.assertFalse(result[key])

    def test_failed_foreign_or_incomplete_operation_rejected(self):
        self.setup_case()
        for key, value in [('status','failed'),('status','running'),('phase','workspace_provision'),
                           ('companyId',OID),('command','other'),('finishedAt',None),('finishedAt','bad')]:
            response = self.response(); response['operation'][key] = value
            with self.assertRaisesRegex(ValueError, '^'+ev.ERROR+'$'):
                ev.stopped(response,[self.workspace],WID,self.config)
        for key in ('action','projectId','projectWorkspaceId','workspaceCommandId','workspaceCommandKind'):
            response = self.response(); response['operation']['metadata'][key] = 'other'
            with self.assertRaises(ValueError): ev.stopped(response,[self.workspace],WID,self.config)

    def test_missing_restarted_changed_exposed_duplicate_rows_rejected(self):
        self.setup_case()
        for key, value in [('status','running'),('id',OID),('command','other'),('url','http://public'),
                           ('exposure',{'state':'cleanup_pending'}),('stoppedAt',None)]:
            w = copy.deepcopy(self.workspace); w['runtimeServices'][0][key] = value
            with self.assertRaises(ValueError): ev.stopped(self.response(),[w],WID,self.config)
        for rows in ([], [self.workspace,self.workspace]):
            with self.assertRaises(ValueError): ev.stopped(self.response(),rows,WID,self.config)
        for services in ([],[self.service,self.service]):
            w = copy.deepcopy(self.workspace); w['runtimeServices'] = services
            with self.assertRaises(ValueError): ev.stopped(self.response(),[w],WID,self.config)

    def test_session_operation_failure_stops_once_and_reconciles_without_polling(self):
        for mode in ('ok','start-failed','stop-failed','stale','io','interrupt'):
            self.setup_case()
            client = Mock(); client._workspace_id = WID
            final = OSError('secret') if mode=='io' else [copy.deepcopy(self.workspace)]
            if mode=='stale': final[0]['runtimeServices'][0]['status'] = 'running'
            client.read_workspaces.side_effect = [[copy.deepcopy(self.case.workspace)],
                [copy.deepcopy(self.case.workspace)], final]
            def request(packet):
                action = packet['path'].rsplit('/',1)[1]
                response = self.response(action)
                if mode == action+'-failed': response['operation']['status'] = 'failed'
                return response
            client.request.side_effect = request
            args = dict(fee_bps=0, broker_root='/private/broker', broker=self.case.broker,
                        audit=self.case.audit, client=client)
            def run():
                with managed_session._observed_session(sys.executable,'/candidate',self.case.argv,**args) as result:
                    if mode=='interrupt': raise KeyboardInterrupt()
                self.assertTrue(result['control_plane_stop_verified'])
                self.assertFalse(result['process_exit_verified'])
            if mode=='ok': run()
            else:
                with self.assertRaises(KeyboardInterrupt if mode=='interrupt' else ValueError): run()
            self.assertEqual([c.args[0]['path'].rsplit('/',1)[1] for c in client.request.call_args_list],['start','stop'])
            self.assertLessEqual(client.read_workspaces.call_count,3)
            self.assertEqual(self.case.events[-1],'broker-close')

if __name__ == '__main__': unittest.main()
