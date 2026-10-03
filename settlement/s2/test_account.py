"""Public DIRECT account reads must fail closed without engine admission."""
import copy
import http.client
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from direct import Direct, chain_owner, route
from server import Server
from test_adapter import fixture, FakeRPC, rehash
from transport import decode, Unavailable


class AccountTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ.get('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(self.temp.cleanup)
        self.snapshot, self.manifest = fixture()
        self.rpc = FakeRPC(self.snapshot)
        self.direct = Direct(self.rpc, self.manifest, Path(self.temp.name))
        self.owner = chain_owner(self.snapshot['body']['accounts'][0]['owner'])
        self.path = '/s2/accounts/' + self.owner

    def test_complete_public_context_without_engine(self):
        status, body = self.direct.request('GET', self.path)
        self.assertEqual(status, 200)
        self.assertTrue(body['signing_ready'])
        self.assertEqual(body['state'], 'COMMITTED')
        self.assertEqual(body['context'], self.manifest['context'])
        self.assertEqual(body['owner'], self.owner)
        for key in ('account_number', 'sequence', 'gas_atoms', 'owner_epoch', 'balances'):
            self.assertEqual(body[key], self.snapshot['body']['accounts'][0][key])
        self.assertEqual(body['observed_height'], body['cursor_height'])
        self.assertTrue(all(params['height'] == '3' for method, params in self.rpc.calls
                            if method in ('block', 'abci_query')))
        self.assertFalse(any(key in body for key in ('reserved', 'pending', 'session', 'orders')))

    def test_stale_future_latency_and_disconnect(self):
        for age in (6000, -2000):
            self.snapshot['body']['block_time_unix_ms'] = str(time.time_ns() // 1_000_000 - age)
            self.assertEqual(self.direct.request('GET', self.path)[0], 503)
        self.snapshot['body']['block_time_unix_ms'] = str(time.time_ns() // 1_000_000)
        with patch('direct.time.monotonic_ns', side_effect=[0, 0, 0, 3_000_000_000]):
            self.assertEqual(self.direct.request('GET', self.path)[0], 503)
        self.rpc.error = 'RPC_UNAVAILABLE'
        status, body = self.direct.request('GET', self.path)
        self.assertEqual(status, 503)
        self.assertFalse(body['signing_ready'])
        self.assertNotIn('balances', body)

    def test_height_mixing_context_and_rpc_loss_at_end(self):
        original = self.rpc.call
        for mode in ('height', 'chain', 'catchup', 'offline'):
            calls = []
            def call(method, params):
                value, raw = original(method, params)
                if method == 'status':
                    calls.append(method)
                    if len(calls) == 2:
                        if mode == 'offline':
                            raise Unavailable('offline')
                        if mode == 'height':
                            value['sync_info']['latest_block_height'] = '4'
                        if mode == 'chain':
                            value['node_info']['network'] = 'other'
                        if mode == 'catchup':
                            value['sync_info']['catching_up'] = True
                return value, raw
            with patch.object(self.rpc, 'call', side_effect=call):
                self.assertEqual(self.direct.request('GET', self.path)[0], 503, mode)
        self.rpc.mutate_block = lambda block: block['block']['header'].update(height='2')
        self.assertEqual(self.direct.request('GET', self.path)[0], 503)

    def test_bad_registered_keys_numbers_assets_and_context(self):
        pristine = copy.deepcopy(self.snapshot)
        mutations = [
            lambda b: b['accounts'][0].update(public_key=b['accounts'][1]['public_key']),
            lambda b: b['accounts'][0].update(public_key_type='other'),
            lambda b: b['accounts'][0].update(sequence='01'),
            lambda b: b['accounts'][0].update(gas_atoms='-1'),
            lambda b: b['accounts'][0].update(account_number=b['accounts'][1]['account_number']),
            lambda b: b['accounts'][0]['balances'][0].update(bank_atoms='0'),
            lambda b: b['accounts'][0]['balances'][0].update(denom='DEVQUOTE'),
            lambda b: b['supplies'][0].update(module_atoms='0'),
            lambda b: b['context'].update(genesis_hash='0'*64),
        ]
        for mutation in mutations:
            self.rpc.snapshot = copy.deepcopy(pristine)
            mutation(self.rpc.snapshot['body'])
            rehash(self.rpc.snapshot)
            self.assertEqual(self.direct.request('GET', self.path)[0], 503)

    def test_unknown_owner_method_and_public_http_boundary(self):
        self.assertEqual(self.direct.request('GET', '/s2/accounts/' + chain_owner('AAAAAAAAAAAAAAAAAAAAAAAAAAA='))[0], 404)
        self.assertEqual(self.direct.request('POST', self.path)[0], 405)
        self.assertFalse(route(self.path + '?height=1'))
        class OfflineEngine:
            def request(self, *args):
                raise Unavailable('offline')
        server = Server(('127.0.0.1', 0), OfflineEngine(), self.direct)
        thread = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': .01})
        thread.start()
        try:
            def request(path, headers):
                conn = http.client.HTTPConnection('127.0.0.1', server.server_port)
                try:
                    conn.request('GET', path, headers=headers)
                    response = conn.getresponse()
                    return response.status, decode(response.read())
                finally:
                    conn.close()
            self.assertEqual(request(self.path, {})[0], 200)
            self.assertEqual(request(self.path, {'Origin':'http://127.0.0.1:5173'})[0], 200)
            self.assertEqual(request(self.path, {'Origin':'https://evil.example'})[0], 403)
            self.assertEqual(request('/s2/me', {})[0], 403)
            self.assertEqual(request('/s2/me', {'Origin':'http://127.0.0.1:5173'})[0], 503)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
