"""DIRECT ingress never treats CheckTx as commitment or depends on engine state."""
import base64
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from direct import Direct
from transport import Unavailable, encode


class DirectTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ.get('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(self.temp.cleanup)
        self.calls = []
        self.raw = b'signed test transaction'
        self.digest = hashlib.sha256(self.raw).hexdigest().upper()
        self.wire = base64.b64encode(self.raw).decode()
        self.result = {'code': 0, 'codespace': '', 'gas_wanted': '500000', 'gas_used': '1000'}
        self.tx = {'tx': self.wire, 'hash': self.digest, 'height': '2', 'index': 0, 'tx_result': self.result.copy()}
        self.block = {'header': {'height': '2', 'chain_id': 'nus-s2-dev-1'}, 'data': {'txs': [self.wire]}}
        self.results = {'height': '2', 'txs_results': [self.result]}
        self.gateway = Direct(self, {'context': {'chain_id': 'nus-s2-dev-1'}}, Path(self.temp.name))
        self.gateway.snapshot = lambda: {'observed_height': '3'}

    def call(self, method, params):
        self.calls.append((method, params))
        return {'broadcast_tx_sync': {'hash': self.digest, 'code': 0},
                'tx': self.tx, 'block': {'block': self.block},
                'block_results': self.results}[method], b''

    def test_submit_persists_exact_bytes_but_only_returns_unknown(self):
        status, body = self.gateway.request('POST', '/s1/txs', encode({'tx_bytes': self.wire}))
        self.assertEqual(status, 202)
        self.assertEqual(body['state'], 'SUBMISSION_UNKNOWN')
        self.assertEqual(body['check_tx_code'], '0')
        self.assertEqual((Path(self.temp.name) / (self.digest + '.tx')).read_bytes(), self.raw)
        self.assertEqual(self.calls, [('broadcast_tx_sync', {'tx': self.wire})])

    def test_evidence_failure_prevents_broadcast(self):
        with patch('direct.durable_file', side_effect=OSError('disk full')):
            status, body = self.gateway.request('POST', '/s1/txs', encode({'tx_bytes': self.wire}))
        self.assertEqual((status, body['state']), (202, 'SUBMISSION_UNKNOWN'))
        self.assertEqual(self.calls, [])

    def test_canonical_body_and_limits(self):
        for raw in (b'{"tx_bytes":"AA==","tx_bytes":"AA=="}', encode({'tx_bytes': ''}),
                    encode({'tx_bytes': 'AB=='}), encode({'tx_bytes': None}),
                    encode({'tx_bytes': self.wire, 'owner': 'other'}),
                    encode({'tx_bytes': base64.b64encode(b'x' * 16385).decode()})):
            self.assertEqual(self.gateway.request('POST', '/s1/txs', raw)[0], 400)
        self.assertEqual(self.calls, [])

    def test_commit_requires_block_and_result_agreement(self):
        self.assertEqual(self.gateway.request('GET', '/s1/txs/' + self.digest)[1]['state'], 'COMMITTED')
        self.tx['tx_result']['code'] = 1
        self.assertEqual(self.gateway.request('GET', '/s1/txs/' + self.digest)[0], 503)
        self.tx['tx_result']['code'] = 0
        self.block['data']['txs'] = []
        self.assertEqual(self.gateway.request('GET', '/s1/txs/' + self.digest)[0], 503)

    def test_unobserved_inclusion_and_rpc_loss_remain_unknown(self):
        self.tx['height'] = '4'
        self.assertEqual(self.gateway.request('GET', '/s1/txs/' + self.digest)[1]['state'], 'SUBMISSION_UNKNOWN')
        with patch.object(self.gateway, 'snapshot', side_effect=Unavailable('offline')):
            self.assertEqual(self.gateway.request('GET', '/s1/txs/' + self.digest)[0], 503)

    def test_other_genesis_cannot_reuse_evidence(self):
        with self.assertRaises(Unavailable):
            Direct(self, {'context': {'chain_id': 'other'}}, Path(self.temp.name))
