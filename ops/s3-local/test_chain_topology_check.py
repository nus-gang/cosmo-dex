import json
from unittest.mock import patch
import unittest
import chain_topology_check as target
from test_chain_preflight import ChainPreflightTest as Fixture

class TopologyCheckTest(unittest.TestCase):
    def setUp(self):
        self.f=Fixture();self.f.setUp();self.addCleanup(self.f.doCleanups)
        self.ids=[str(i)*40 for i in range(4)]
        self.report=json.dumps(dict(node_ids=self.ids,approval_verified=False,
            port_availability_verified=False,writer_exclusion_verified=False),separators=(',',':')).encode()+b'\n'
    def call(self,body,**kw):
        stage=self.f.staged(body)
        with patch.object(target,'preflight_payload',return_value=b'[["preflight"]]'):
            return target.check('/python','/candidate',[],{'node_ids':self.ids},stage,
                fee_bps=0,scratch=self.f.root,**kw)
    def test_exact_file_mode_argv_report_cleanup(self):
        result=self.call('import os,sys,stat\nfrom pathlib import Path\n'
            'assert sys.argv[1:3]==["topology","--topology"]\n'
            'p=Path(sys.argv[3]);assert p.read_bytes()==b\'[["preflight"]]\'\n'
            'assert stat.S_IMODE(p.stat().st_mode)==0o600\n'
            'assert stat.S_IMODE(p.parent.stat().st_mode)==0o700\n'
            'assert sys.stdin.buffer.read()==b""\n'
            'sys.stdout.buffer.write('+repr(self.report)+')')
        self.assertTrue(result['home_identity_verified']);self.assertFalse(result['approval_verified'])
        self.assertEqual(list(self.f.root.glob('chain-topology-*')),[])
    def test_mutation_wrong_ids_timeout_cleanup(self):
        for body in ['print("wrong")', 'import time;time.sleep(10)',
            'import sys;from pathlib import Path;Path(sys.argv[3]).write_bytes(b"changed");sys.stdout.buffer.write('+repr(self.report)+')']:
            with self.assertRaises(ValueError):self.call(body,timeout=.3)
            self.assertEqual(list(self.f.root.glob('chain-topology-*')),[])
    def test_stop_interrupt_and_fsync_failure_cleanup(self):
        with self.assertRaises(ValueError):self.call('pass',stopped=lambda:True)
        def stop():raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):self.call('pass',stopped=stop)
        with patch.object(target.os,'fsync',side_effect=OSError('disk')):
            with self.assertRaises(OSError):self.call('pass')
        self.assertEqual(list(self.f.root.glob('chain-topology-*')),[])

if __name__=='__main__':unittest.main()
