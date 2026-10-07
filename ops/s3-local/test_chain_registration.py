import copy
import shlex
import sys
import unittest
import test_chain_cli as fixtures
import workspace_registration as r
from native_review import COMPANY

class ChainRegistrationTest(unittest.TestCase):
    def args(self):
        return fixtures.ChainCliTest.args(self)[1:]

    def test_eight_manual_packets_exact_argv(self):
        names=set()
        for fee in (0,25):
            for index in range(4):
                args=self.args()
                args[args.index('--home')+1]="/private/home '$()`; literal"
                packet=r.prepare_chain(sys.executable,'/candidate',args,fee_bps=fee,validator_index=index)
                body=packet['body']; names.add(body['name'])
                config=body['runtimeConfig']; command=config['workspaceRuntime']['commands'][0]
                self.assertEqual(shlex.split(command['command']),['exec',sys.executable,'-B','/candidate/ops/s3-local/chain_cli.py','serve-chain-reviewed',*args])
                self.assertEqual(command['port'],26657)
                self.assertEqual(config['desiredState'],'manual')
                self.assertEqual(config['serviceStates'],{'0':'manual'})
                self.assertNotIn('env',command); self.assertNotIn('expose',command)
                self.assertTrue(packet['requires_board_registration'])
                self.assertFalse(packet['starts_service']); self.assertFalse(packet['approval_verified'])
                w=dict(copy.deepcopy(body),id='11111111-1111-4111-8111-111111111111',companyId=COMPANY,projectId=r.PROJECT,runtimeServices=[])
                got=r.registered(packet,[w])
                self.assertEqual(got['requests']['start']['body'],{'workspaceCommandId':f's3-chain-fee{fee}-v{index}'})
                self.assertEqual(got['requests']['stop']['body'],got['requests']['start']['body'])
        self.assertEqual(len(names),8)

    def test_bad_inputs_rejected(self):
        for fee,index in [(True,0),(1,0),(0,True),(0,-1),(0,4)]:
            with self.assertRaises(ValueError):r.prepare_chain(sys.executable,'/candidate',self.args(),fee_bps=fee,validator_index=index)
        for option,value in [('--rpc','0.0.0.0:26657'),('--p2p','127.0.0.1:26657'),('--peers',''),('--home','relative'),('--home','/private/{{secret}}'),('--lifetime-seconds','301')]:
            args=self.args();args[args.index(option)+1]=value
            with self.assertRaises(ValueError):r.prepare_chain(sys.executable,'/candidate',args,fee_bps=0,validator_index=0)
        args=self.args();args.remove('--acknowledge-unproven-space')
        with self.assertRaises(ValueError):r.prepare_chain(sys.executable,'/candidate',args,fee_bps=0,validator_index=0)

    def test_registration_drift_duplicate_and_services_rejected(self):
        packet=r.prepare_chain(sys.executable,'/candidate',self.args(),fee_bps=0,validator_index=0)
        w=dict(copy.deepcopy(packet['body']),id='11111111-1111-4111-8111-111111111111',companyId=COMPANY,projectId=r.PROJECT,runtimeServices=[])
        for field,value in [('runtimeServices',[{}]),('isPrimary',True),('cwd','/other')]:
            changed=copy.deepcopy(w);changed[field]=value
            with self.assertRaises(ValueError):r.registered(packet,[changed])
        for listing in ([],[w,w]):
            with self.assertRaises(ValueError):r.registered(packet,listing)
        for field,value in [('command','changed'),('port',26658),('id','s3-chain-fee0-v4')]:
            changed=copy.deepcopy(w);changed['runtimeConfig']['workspaceRuntime']['commands'][0][field]=value
            with self.assertRaises(ValueError):r.registered(packet,[changed])

if __name__=='__main__':unittest.main()
