import copy
import unittest
import sys
from registration_render import render,ERROR


def fixture():
    spec=dict(schema='s3-local-initializer-registration/1',python=sys.executable,
        candidate='/candidate',bundle='/candidate-bundle',artifacts='/artifacts',runtime_pin='a'*64,
        native_decision_id='11111111-2222-4333-8444-555555555555',
        ceo_revision='22222222-2222-4333-8444-555555555555',cto_revision='33333333-2222-4333-8444-555555555555',
        control_root='/private/control',profiles={'fee0':'/private/fresh/fee0','fee25':'/private/fresh/fee25'})
    reports={}
    for fee in (0,25):
        root=spec['profiles']['fee'+str(fee)]
        reports['fee'+str(fee)]=dict(schema='s3-local-initialization/1',fee_bps=str(fee),root=root,
            authority_root=root+'/authority',homes=[root+'/validator-'+str(i) for i in range(4)],
            node_ids=[format(fee+i+1,'040x') for i in range(4)],genesis_sha256=('b' if fee==0 else 'c')*64,
            guard_sha256='d'*64,runtime_pin=spec['runtime_pin'],c_semantic_validation_verified=True,service_started=False)
    return spec,reports


class RenderTest(unittest.TestCase):
    def test_twelve_commands_from_exact_reports(self):
        spec,reports=fixture();value=render(spec,reports)
        self.assertEqual(value,render(spec,reports));self.assertEqual(value['packet_count'],12)
        self.assertFalse(value['starts_service'])
        for packet in value['packets']:
            command=packet['body']['runtimeConfig']['workspaceRuntime']['commands'][0]
            self.assertNotIn('<',command['command'])
            self.assertIn('--native-decision-id',command['command'])
        worker=value['packets'][0]['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command']
        self.assertIn('/private/fresh/fee0/authority/operator-0',worker)
        self.assertIn('/private/fresh/fee0/input.json',worker)
        self.assertEqual(value['execution_order'],['fee0','stop-and-verify-release','fee25'])

    def test_reused_identity_wrong_home_pin_and_fee_rejected(self):
        spec,reports=fixture()
        for key,value in [('runtime_pin','b'*64),('fee_bps','0'),('root','/other'),
                           ('homes',reports['fee0']['homes']),('node_ids',reports['fee0']['node_ids']),
                           ('genesis_sha256',reports['fee0']['genesis_sha256']),('service_started',True)]:
            changed=copy.deepcopy(reports);changed['fee25'][key]=value
            with self.subTest(key=key),self.assertRaisesRegex(ValueError,ERROR):render(spec,changed)

    def test_control_overlap_and_unix_limit_rejected(self):
        for root in ('/private/fresh/fee0/control','/private/'+'x'*100):
            spec,reports=fixture();spec['control_root']=root
            with self.assertRaisesRegex(ValueError,ERROR):render(spec,reports)


if __name__=='__main__':unittest.main()
