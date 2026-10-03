"""DIRECT ingress never treats CheckTx as commitment or depends on engine state."""
import base64
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from direct import Direct, chain_owner
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

    def test_receipt_binding_not_found_and_rpc_failure(self):
        encoded_owner = 'WqVsIIzDjWIW8PDv6e4bRsyh+44='
        owner, rid = chain_owner(encoded_owner), '1' * 64
        self.assertEqual(owner, 'nus1t2jkcgyvcwxky9hs7rh7nmsmgmx2r7uwsvvqu7')
        self.gateway.manifest.update(owners=[encoded_owner])
        self.gateway.manifest['context']['genesis_hash'] = 'a' * 64
        receipt = dict(chain_id='nus-s2-dev-1', genesis_hash='a' * 64,
                       owner=owner, request_id=rid, committed_height='2',
                       state='COMMITTED', original_tx_hash=self.digest)
        response = {'height': '3', 'code': 0}
        path = '/s1/accounts/' + owner + '/requests/' + rid
        def query(method, params):
            self.assertEqual(method, 'abci_query')
            self.assertEqual(params['height'], '3')
            self.assertEqual(bytes.fromhex(params['data']),
                             bytes((10, len(owner))) + owner.encode() + bytes((18, 64)) + rid.encode())
            return {'response': response}, encode(response)
        with patch.object(self.gateway.rpc, 'call', side_effect=query), patch('direct.json_response', return_value=receipt):
            unknown_path = '/s1/accounts/' + chain_owner('AAAAAAAAAAAAAAAAAAAAAAAAAAA=') + '/requests/' + rid
            self.assertEqual(self.gateway.request('GET', unknown_path)[0], 404)
            response['value'] = 'AA=='
            self.assertEqual(self.gateway.request('GET', path), (200, receipt))
            for key, bad in [('owner', 'other'), ('genesis_hash', 'b' * 64),
                             ('chain_id', 'other'), ('request_id', '2' * 64),
                             ('committed_height', '4'), ('state', 'PENDING')]:
                old = receipt[key]
                receipt[key] = bad
                self.assertEqual(self.gateway.request('GET', path)[0], 503)
                receipt[key] = old
            response['height'] = '4'
            self.assertEqual(self.gateway.request('GET', path)[0], 503)
            response.update(height='3', code=1, log='NOT_FOUND_AT_HEIGHT 3')
            self.assertEqual(self.gateway.request('GET', path)[1]['state'], 'NOT_FOUND_AT_HEIGHT')
            response['log'] = 'NOT_FOUND_AT_HEIGHT 2'
            self.assertEqual(self.gateway.request('GET', path)[0], 503)
        with patch.object(self.gateway, 'snapshot', side_effect=Unavailable('offline')):
            self.assertEqual(self.gateway.request('GET', path)[0], 503)
