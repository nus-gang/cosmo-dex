import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import chain_topology as t
from chain_stage import StagedChain
from test_chain_topology import nodes

class PayloadTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        root=Path(self.tmp.name)
        for name,raw in [('chain',b'binary'),('input',b'capture'),('profile',b'profile')]:
            (root/name).write_bytes(raw)
        sha=lambda b:hashlib.sha256(b).hexdigest()
        self.stage=StagedChain(root/'chain',root/'input',root/'profile',sha(b'binary'),sha(b'capture'),sha(b'profile'))
        self.nodes=nodes()
        self.verify=patch.object(StagedChain,'verify',autospec=True).start()
        self.capture=patch('preflight.verify_input_set',return_value=(b'capture',{})).start()
        patch('preflight.checked_root',side_effect=lambda x:x).start()
        self.profile=patch('preflight.bounded',side_effect=lambda root,name,limit: (root/name).read_bytes() if root==self.stage.input_set.parent else b'profile').start()
        self.addCleanup(patch.stopall)
    def call(self,fee=0,packet=None):
        packet=t.prepare(sys.executable,'/candidate',self.nodes,fee_bps=fee) if packet is None else packet
        return t.preflight_payload(sys.executable,'/candidate',self.nodes,packet,self.stage,fee_bps=fee)
    def test_exact_go_argv_and_immutable_output(self):
        for fee in (0,25):
            raw=self.call(fee);args=json.loads(raw)
            self.assertEqual(len(args),4)
            for i,a in enumerate(args):
                self.assertEqual(a[0],'preflight')
                self.assertEqual(a[a.index('--input-set')+1],str(self.stage.input_set))
                self.assertEqual(a[a.index('--local-demo-profile')+1],str(self.stage.effective_profile))
                self.assertEqual(a[a.index('--home')+1],f'/private/homes/v{i}')
                self.assertNotIn('--approval-socket',a);self.assertNotIn('start',a)
            self.assertEqual(raw,self.call(fee))
        old=raw;self.nodes[0]['argv'].append('--bad');self.assertEqual(raw,old)
    def test_packet_change_rejected_before_stage_or_capture(self):
        p=t.prepare(sys.executable,'/candidate',self.nodes,fee_bps=0)
        p['packets'][0]['body']['name']='changed'
        with self.assertRaisesRegex(ValueError,t.ERROR):self.call(packet=p)
        self.verify.assert_not_called();self.capture.assert_not_called()
    def test_capture_profile_and_stage_changes_rejected(self):
        self.capture.return_value=(b'other',{})
        with self.assertRaisesRegex(ValueError,t.ERROR):self.call()
        self.capture.return_value=(b'capture',{})
        self.profile.side_effect=lambda root,name,limit: (root/name).read_bytes() if root==self.stage.input_set.parent else b'other'
        with self.assertRaisesRegex(ValueError,t.ERROR):self.call()
        self.profile.side_effect=lambda root,name,limit: (root/name).read_bytes() if root==self.stage.input_set.parent else b'profile'
        self.stage.input_set.write_bytes(b'other')
        with self.assertRaisesRegex(ValueError,t.ERROR):self.call()
        self.stage.input_set.write_bytes(b'capture')
        self.verify.side_effect=[None,ValueError('changed')]
        with self.assertRaisesRegex(ValueError,t.ERROR):self.call()

if __name__=='__main__':unittest.main()
