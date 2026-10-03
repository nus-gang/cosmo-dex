"""Bounded transport tests; synthetic RPC is not real-chain S2 acceptance."""
import base64
import copy
import datetime
import hashlib
import http.client
import os
from pathlib import Path
import struct
import sys
import tempfile
import threading
import time
import unittest

from chain import Collector, json_response, loopback_url
from server import Server
from transport import Engine, Unavailable, decode, encode

ROOT = Path(__file__).resolve().parents[2]


def rehash(snapshot):
    domain = b'NUS/S2/SNAPSHOT/V1'
    raw = encode(snapshot['body'])
    snapshot['snapshot_id'] = hashlib.sha256(
        struct.pack('>I', len(domain)) + domain + struct.pack('>Q', len(raw)) + raw).hexdigest()
    return snapshot


def fixture():
    snapshot = decode((ROOT / 'protocol/s2/vectors/snapshot.json').read_bytes())['snapshot']
    pin = decode((ROOT / 'protocol/s2/manifest.json').read_bytes())
    snapshot['body']['context'].update(contract_hash=pin['contract_sha256'], config_hash=pin['config_sha256'])
    snapshot['body']['observed_height'] = '1'
    snapshot['body']['block_time_unix_ms'] = str(time.time_ns() // 1_000_000)
    rehash(snapshot)
    body = snapshot['body']
    manifest = {'context': body['context'], 'market': body['market'],
                'owners': [a['owner'] for a in body['accounts']],
                'supplies': [s['genesis_supply_atoms'] for s in body['supplies']],
                'bootstrap_snapshot_id': snapshot['snapshot_id']}
    return snapshot, manifest


def proto_json(value):
    raw = encode(value)
    size = len(raw)
    length = bytearray()
    while size > 127:
        length.append((size & 127) | 128)
        size >>= 7
    length.append(size)
    return b'\x0a' + length + raw


class FakeRPC:
    def __init__(self, snapshot, tip=3):
        self.snapshot = snapshot
        self.tip = tip
        self.calls = []
        self.error = None
        self.mutate_block = lambda x: None

    def call(self, method, params):
        self.calls.append((method, params))
        if self.error:
            raise Unavailable(self.error)
        if method == 'status':
            result = {'node_info': {'network': 'nus-s2-dev-1'},
                      'sync_info': {'latest_block_height': str(self.tip), 'catching_up': False}}
        else:
            snapshot = copy.deepcopy(self.snapshot)
            body = snapshot['body']
            body['observed_height'] = params['height']
            rehash(snapshot)
            if method == 'abci_query':
                result = {'response': {'height': params['height'], 'code': 0,
                                       'value': base64.b64encode(proto_json(snapshot)).decode()}}
            else:
                timestamp = datetime.datetime.fromtimestamp(
                    int(body['block_time_unix_ms']) / 1000, datetime.timezone.utc)
                result = {'block_id': {'hash': body['block_hash'].upper()},
                          'block': {'header': {'height': params['height'],
                                              'chain_id': 'nus-s2-dev-1',
                                              'time': timestamp.isoformat(timespec='milliseconds').replace('+00:00', 'Z')}}}
                self.mutate_block(result)
        return result, encode({'jsonrpc': '2.0', 'id': 1, 'result': result})


class FakeEngine:
    def __init__(self):
        self.height = 1
        self.messages = []
        self.failed = 0
        self.body = None

    def request(self, method, path, origin=None, authorization=None, body=b''):
        self.body = body
        return 200, {'observation': {'observed_height': str(self.height)}}

    def call(self, message):
        self.messages.append(message)
        self.height = int(message['cursor_height'])
        return 200, {}

    def rpc_failed(self):
        self.failed += 1
        return 200, {}


class AdapterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ.get('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)

    def test_duplicate_json_and_protobuf_trailing_bytes_rejected(self):
        for raw in (b'{"a":1,"a":2}', b'{"n":NaN}'):
            with self.assertRaises(ValueError):
                decode(raw)
        for raw in (b'\x0a\x82\x00{}', proto_json({}) + b'x', b'\x12\x02{}'):
            with self.assertRaises(Unavailable):
                json_response(raw)

    def test_rpc_url_has_no_dns_proxy_redirect_target(self):
        self.assertEqual(loopback_url('http://127.0.0.1:26657'), 'http://127.0.0.1:26657')
        for value in ('http://localhost:26657', 'http://127.0.0.1:26657@evil.test:80',
                      'http://127.0.0.1:26657/path', 'http://192.168.0.1:26657',
                      'http://127.0.0.1:26657?target=x'):
            with self.assertRaises(ValueError):
                loopback_url(value)

    def test_cursor_reobserves_and_consumes_each_empty_height(self):
        snapshot, manifest = fixture()
        engine = FakeEngine()
        collector = Collector(FakeRPC(snapshot, 12), engine, manifest, self.path)
        self.assertTrue(collector.tick())
        self.assertEqual([m['cursor_height'] for m in engine.messages], list(map(str, range(1, 9))))
        self.assertTrue(all(m['catching_up'] for m in engine.messages))
        engine.messages.clear()
        self.assertTrue(collector.tick())
        self.assertEqual([m['cursor_height'] for m in engine.messages], list(map(str, range(8, 13))))
        self.assertFalse(engine.messages[-1]['catching_up'])
        self.assertEqual(len(list(self.path.glob('*.json'))), 12)

    def test_disconnect_preserves_cursor_and_retries_missing_height(self):
        snapshot, manifest = fixture()
        rpc, engine = FakeRPC(snapshot), FakeEngine()
        collector = Collector(rpc, engine, manifest, self.path)
        rpc.error = 'RPC_UNAVAILABLE'
        self.assertFalse(collector.tick())
        self.assertEqual(engine.height, 1)
        self.assertEqual(engine.failed, 1)
        rpc.error = None
        self.assertTrue(collector.tick())
        self.assertEqual([m['cursor_height'] for m in engine.messages], ['1', '2', '3'])

    def test_regression_is_latched_until_explicit_restart(self):
        snapshot, manifest = fixture()
        rpc, engine = FakeRPC(snapshot, 0), FakeEngine()
        collector = Collector(rpc, engine, manifest, self.path)
        self.assertFalse(collector.tick())
        rpc.tip = 3
        self.assertFalse(collector.tick())
        self.assertEqual(collector.last_error, 'HEIGHT_REGRESSION')
        self.assertFalse(engine.messages)

    def test_header_hash_time_height_chain_mismatch_never_observed(self):
        for target, value in [('hash', 'FF' * 32), ('height', '99'),
                              ('chain_id', 'nus-s1-dev-1'), ('time', '2026-10-01T00:00:00Z')]:
            with self.subTest(target=target):
                snapshot, manifest = fixture()
                rpc, engine = FakeRPC(snapshot), FakeEngine()
                def mutate(block):
                    (block['block_id'] if target == 'hash' else block['block']['header'])[target] = value
                rpc.mutate_block = mutate
                collector = Collector(rpc, engine, manifest, self.path)
                self.assertFalse(collector.tick())
                self.assertFalse(engine.messages)
                self.assertEqual(engine.height, 1)
                self.assertEqual(collector.last_error, 'BLOCK_BINDING')

    def test_failed_evidence_write_closes_admission(self):
        snapshot, manifest = fixture()
        engine = FakeEngine()
        collector = Collector(FakeRPC(snapshot), engine, manifest, self.path)
        collector.evidence = self.path / 'missing-directory'
        self.assertFalse(collector.tick())
        self.assertFalse(engine.messages)
        self.assertGreater(engine.failed, 0)

    def child(self, code, timeout=2):
        engine = Engine([sys.executable, '-u', '-c', code], timeout)
        self.addCleanup(engine.close)
        return engine

    def test_pipe_preserves_raw_request_and_serializes_threads(self):
        engine = self.child('import sys,json\nfor line in sys.stdin:\n v=json.loads(line)\n print(json.dumps({"http_status":"200","body":v}),flush=True)')
        raw = b'{"op":"observe", "snapshot":{},"a":1,"a":2}'
        status, body = engine.request('POST', '/s2/orders', body=raw)
        self.assertEqual(status, 200)
        self.assertEqual(body['op'], 'request')
        self.assertEqual(base64.b64decode(body['body_base64']), raw)
        results = {}
        def submit(i):
            results[i] = engine.request('GET', f'/s2/{i}')[1]['path']
        threads = [threading.Thread(target=submit, args=(i,)) for i in range(8)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        self.assertEqual(results, {i: f'/s2/{i}' for i in range(8)})

    def test_pipe_timeout_eof_malformed_are_permanent_unknown(self):
        for code in ('import time; time.sleep(10)',
                     'import sys; sys.stdin.readline()',
                     'import sys; sys.stdin.readline(); print("{}",flush=True)'):
            with self.subTest(code=code):
                engine = self.child(code, .2)
                with self.assertRaises(Unavailable):
                    engine.request('POST', '/s2/orders', body=b'{}')
                self.assertIsNotNone(engine.child.poll())
                with self.assertRaises(Unavailable):
                    engine.request('GET', '/s2/status')

    def test_http_cors_body_limit_control_isolation_cache_and_duplicate_headers(self):
        engine = FakeEngine()
        server = Server(('127.0.0.1', 0), engine)
        thread = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': .01})
        thread.start()
        try:
            def request(method, path, body=None, headers=None):
                connection = http.client.HTTPConnection('127.0.0.1', server.server_port, timeout=2)
                try:
                    connection.request(method, path, body, headers or {})
                    response = connection.getresponse()
                    data = response.read()
                    return response.status, dict(response.getheaders()), data
                finally:
                    connection.close()
            origin = 'http://127.0.0.1:5173'
            self.assertEqual(request('GET', '/s2/me')[0], 403)
            self.assertEqual(request('GET', '/s2/status', headers={'Host': 'evil.test'})[0], 403)
            self.assertEqual(request('GET', '/s2/status', headers={'Origin': 'http://evil.test'})[0], 403)
            self.assertEqual(request('GET', '/s2/observe')[0], 403)
            status, headers, _ = request('GET', '/s2/status')
            self.assertEqual(status, 200)
            self.assertEqual(headers['Cache-Control'], 'no-store')
            self.assertNotIn('Access-Control-Allow-Origin', headers)
            status, headers, _ = request('OPTIONS', '/s2/orders', headers={
                'Origin': origin, 'Access-Control-Request-Method': 'POST',
                'Access-Control-Request-Headers': 'content-type, authorization'})
            self.assertEqual(status, 204)
            self.assertEqual(headers['Access-Control-Allow-Origin'], origin)
            headers = {'Origin': origin, 'Content-Type': 'application/json'}
            raw = b'{"op":"observe","snapshot":{}}'
            self.assertEqual(request('POST', '/s2/orders', raw, headers)[0], 200)
            self.assertEqual(engine.body, raw)
            self.assertEqual(request('POST', '/s2/orders', b' ' * 16385, headers)[0], 413)
            connection = http.client.HTTPConnection('127.0.0.1', server.server_port, timeout=2)
            try:
                connection.putrequest('POST', '/s2/orders')
                connection.putheader('Origin', origin)
                connection.putheader('Content-Length', '0')
                connection.putheader('Content-Length', '20')
                connection.endheaders()
                self.assertEqual(connection.getresponse().status, 400)
            finally:
                connection.close()
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    @unittest.skipUnless(os.environ.get('S2_ENGINE_BINARY'), 'set S2_ENGINE_BINARY for real Rust pipe')
    def test_real_engine_rpc_cursor_restart_and_freshness(self):
        snapshot, manifest = fixture()
        (self.path / 'manifest.json').write_bytes(encode(manifest))
        (self.path / 'bootstrap.json').write_bytes(encode(snapshot))
        args = [os.environ['S2_ENGINE_BINARY'], 'create', str(self.path / 'manifest.json'),
                str(self.path / 'journal'), str(self.path / 'bootstrap.json')]
        engine = Engine(args)
        self.addCleanup(engine.close)
        rpc = FakeRPC(snapshot)
        collector = Collector(rpc, engine, manifest, self.path / 'evidence')
        self.assertEqual(engine.request('GET', '/s2/status')[1]['mode'], 'CATCHING_UP')
        self.assertTrue(collector.tick(), collector.last_error)
        status, before = engine.request('GET', '/s2/status')
        self.assertEqual(status, 200)
        self.assertEqual(before['mode'], 'OPEN')
        self.assertEqual(before['observation']['observed_height'], '3')
        engine.close()
        engine = Engine([args[0], 'open', args[2], args[3]])
        self.addCleanup(engine.close)
        reopened = engine.request('GET', '/s2/status')[1]
        self.assertEqual(reopened['mode'], 'CATCHING_UP')
        self.assertEqual(reopened['stream_seq'], before['stream_seq'])
        self.assertGreater(int(reopened['revision']), int(before['revision']))
        self.assertEqual(reopened['observation']['observed_height'], '3')
        collector = Collector(rpc, engine, manifest, self.path / 'evidence')
        self.assertTrue(collector.tick(), collector.last_error)
        rpc.error = 'RPC_UNAVAILABLE'
        self.assertFalse(collector.tick())
        after = engine.request('GET', '/s2/status')[1]
        self.assertEqual(after['mode'], 'STALE')
        self.assertEqual(after['observation']['observed_height'], '3')
        self.assertEqual(after['stream_seq'], before['stream_seq'])
        rpc.error = None
        rpc.tip = 2
        self.assertFalse(collector.tick())
        after = engine.request('GET', '/s2/status')[1]
        self.assertEqual(after['mode'], 'RECOVERY_REQUIRED')
        self.assertEqual(after['reason'], 'HEIGHT_REGRESSION')
        self.assertEqual(after['observation']['observed_height'], '3')
        self.assertEqual(after['stream_seq'], before['stream_seq'])


if __name__ == '__main__':
    unittest.main()
