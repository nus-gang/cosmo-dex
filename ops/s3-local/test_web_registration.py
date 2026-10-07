import copy
import shlex
import sys
import unittest
import test_web_cli as fixtures
import workspace_registration as registration
from native_review import COMPANY

class WebRegistrationTest(unittest.TestCase):
    def args(self):
        return fixtures.WebCliTest.args(self)[1:]

    def test_manual_exact_commands_and_registration(self):
        for fee in (0, 25):
            args = self.args()
            args[args.index('--home')+1] = "/private/home '$()`; literal"
            packet = registration.prepare_web(sys.executable, '/candidate', args, fee_bps=fee)
            body = packet['body']; config = body['runtimeConfig']
            command = config['workspaceRuntime']['commands'][0]
            self.assertEqual(shlex.split(command['command'])[5:], args)
            self.assertEqual(command['port'], 5173)
            self.assertEqual(config['desiredState'], 'manual')
            self.assertEqual(config['serviceStates'], {'0':'manual'})
            self.assertNotIn('env', command); self.assertNotIn('expose', command)
            w = dict(copy.deepcopy(body), id='11111111-1111-4111-8111-111111111111',
                     companyId=COMPANY, projectId=registration.PROJECT, runtimeServices=[])
            matched = registration.registered(packet, [w])
            self.assertEqual(matched['requests']['start']['body'],
                             {'workspaceCommandId':f's3-web-fee{fee}'})
            w['runtimeConfig']['workspaceRuntime']['commands'][0]['command'] += ' changed'
            with self.assertRaises(ValueError): registration.registered(packet, [w])

    def test_bad_web_inputs_fail_before_effects(self):
        for option, value in [('--web-origin','http://example.org:5173'),
                ('--bind','0.0.0.0:8787'),('--bind','127.0.0.1:5173'),
                ('--approval-socket','relative'),('--runtime-pin','a'*40),
                ('--lifetime-seconds','301'),('--home','/private/{{secret}}')]:
            args=self.args(); args[args.index(option)+1]=value
            with self.assertRaises(ValueError):
                registration.prepare_web(sys.executable,'/candidate',args,fee_bps=0)
        with self.assertRaises(ValueError):
            registration.prepare_web(sys.executable,'/candidate',self.args(),fee_bps=True)
        args=self.args(); args.remove('--acknowledge-unproven-space')
        with self.assertRaises(ValueError):
            registration.prepare_web(sys.executable,'/candidate',args,fee_bps=0)

if __name__ == '__main__': unittest.main()
