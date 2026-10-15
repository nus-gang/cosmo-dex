import copy
import sys
import unittest
from test_runtime_config import RuntimeConfigTest as Fixtures
from native_review import COMPANY
import workspace_registration as registration


class RegistrationTest(unittest.TestCase):
    def fixture(self, fee=0):
        packet = registration.prepare(sys.executable, '/candidate', Fixtures.args(self), fee_bps=fee)
        workspace = dict(copy.deepcopy(packet['body']),
            id='11111111-1111-4111-8111-111111111111', companyId=COMPANY,
            projectId=registration.PROJECT, runtimeServices=[])
        return packet, workspace

    def test_fee_requests_and_detached_response(self):
        for fee in (0, 25):
            packet, workspace = self.fixture(fee)
            result = registration.registered(packet, [workspace])
            self.assertFalse(result['approval_verified'])
            self.assertEqual(set(result['requests']), {'start', 'stop'})
            for action, request in result['requests'].items():
                self.assertTrue(request['path'].endswith('/'+action))
                self.assertEqual(request['body'], {'workspaceCommandId':f's3-worker-fee{fee}'})
            workspace['runtimeConfig'].clear()
            self.assertTrue(result['registered_config'])

    def test_primary_foreign_active_and_command_drift_refused(self):
        packet, good = self.fixture()
        variants = []
        for key, value in [('companyId','other'),('projectId','other'),('id','../x'),
                ('isPrimary',True),('cwd','/shared'),('sourceType','git_repo'),
                ('runtimeServices',[{'status':'stopped'}]),('setupCommand','echo bad'),
                ('cleanupCommand','echo bad'),('sharedWorkspaceKey','shared'),
                ('remoteProvider','remote'),('repoUrl','https://example.org')]:
            w = copy.deepcopy(good); w[key] = value; variants.append(w)
        for key in ('command','env'):
            w = copy.deepcopy(good)
            w['runtimeConfig']['workspaceRuntime']['commands'][0][key] = 'changed'
            variants.append(w)
        w = copy.deepcopy(good); w['runtimeConfig']['desiredState']='running'; variants.append(w)
        for w in variants:
            with self.assertRaisesRegex(ValueError, registration.ERROR):
                registration.registered(packet,[w])

    def test_ambiguous_missing_and_wrong_route_refused(self):
        packet, w = self.fixture()
        for listing in ([],[w,w],None,[{}]):
            with self.assertRaisesRegex(ValueError, registration.ERROR):
                registration.registered(packet,listing)
        for key, value in [('path','/api/companies/other'),('method','PATCH'),
                           ('approval_verified',True),('requires_board_registration',False)]:
            p = copy.deepcopy(packet); p[key]=value
            with self.assertRaisesRegex(ValueError, registration.ERROR):
                registration.registered(p,[w])


if __name__ == '__main__': unittest.main()
