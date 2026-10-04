"""Genesis binding tests; synthetic RPC is not real-chain acceptance."""
import copy
import hashlib
import os
from pathlib import Path
import tempfile
import unittest

from bootstrap import genesis_manifest, prepare
from test_adapter import FakeRPC, fixture, rehash
from transport import Unavailable, encode


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.getenv('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.snapshot, _ = fixture()
        self.genesis = {'chain_id': 'nus-s2-dev-1', 'initial_height': '1', 'app_state': {
            'public_keys': [a['public_key'] for a in self.snapshot['body']['accounts']]}}
        self.path = self.root / 'genesis.json'
        self.path.write_bytes(encode(self.genesis))
        manifest, _ = genesis_manifest(self.path.read_bytes())
        self.snapshot['body']['context'] = manifest['context']
        rehash(self.snapshot)

    def test_exact_bytes_and_exclusive_publish(self):
        output = self.root / 'engine'
        manifest = prepare(self.path, output, FakeRPC(self.snapshot))
        self.assertEqual(manifest['context']['genesis_hash'], hashlib.sha256(self.path.read_bytes()).hexdigest())
        self.assertEqual((output / 'genesis.json').read_bytes(), self.path.read_bytes())
        self.assertEqual(manifest['bootstrap_snapshot_id'], self.snapshot['snapshot_id'])
        with self.assertRaises(FileExistsError):
            prepare(self.path, output, FakeRPC(self.snapshot))

    def test_wrong_chain_height_duplicate_key(self):
        for change in ({'chain_id': 'nus-s1-dev-1'}, {'initial_height': '2'},
                       {'app_state': {'public_keys': self.genesis['app_state']['public_keys'][:1] * 2}}):
            value = copy.deepcopy(self.genesis)
            value.update(change)
            with self.assertRaises(Unavailable):
                genesis_manifest(encode(value))

    def test_snapshot_cannot_select_genesis_keys_supply_or_context(self):
        for index, field in enumerate(('key', 'supply', 'context')):
            snapshot = copy.deepcopy(self.snapshot)
            body = snapshot['body']
            if field == 'key':
                body['accounts'][0]['public_key'] = body['accounts'][1]['public_key']
            elif field == 'supply':
                body['supplies'][0]['genesis_supply_atoms'] = '1'
            else:
                body['context']['genesis_hash'] = '0' * 64
            rehash(snapshot)
            output = self.root / str(index)
            with self.assertRaises(Unavailable):
                prepare(self.path, output, FakeRPC(snapshot))
            self.assertFalse((output / 'manifest.json').exists())
