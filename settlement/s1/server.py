#!/usr/bin/env python3
"""Local S1 REST gateway. The committed chain is the only balance authority."""
import argparse
import datetime
import base64
import hashlib
import json
import re
import sqlite3
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Unavailable(Exception):
    pass


def field(tag, value):
    value = value.encode()
    n = len(value)
    length = bytearray()
    while n > 127:
        length.append((n & 127) | 128)
        n >>= 7
    return bytes([tag << 3 | 2]) + length + bytes([n]) + value


def decode_json_response(raw):
    if not raw or raw[0] != 10:
        raise Unavailable('invalid QueryJSONResponse')
    n = shift = 0
    for i in range(1, min(len(raw), 11)):
        n |= (raw[i] & 127) << shift
        if raw[i] < 128:
            if i + 1 + n != len(raw):
                break
            return json.loads(raw[i + 1:])
        shift += 7
    raise Unavailable('invalid protobuf length')


def integer(v):
    if not isinstance(v, str) or not re.fullmatch(r'0|[1-9][0-9]*', v):
        raise Unavailable('noncanonical integer from RPC')
    return int(v)


class Gateway:
    def __init__(self, rpc, genesis_hash, db):
        if not re.fullmatch('[0-9a-f]{64}', genesis_hash):
            raise ValueError('pinned genesis SHA256 required')
        self.rpc_url = rpc
        self.genesis_hash = genesis_hash
        self.db = db
        with self.connect() as c:
            c.execute('CREATE TABLE IF NOT EXISTS network (genesis TEXT PRIMARY KEY)')
            rows = c.execute('SELECT genesis FROM network').fetchall()
            if rows and rows != [(genesis_hash,)]:
                raise ValueError('journal belongs to another genesis')
            c.execute('INSERT OR IGNORE INTO network VALUES (?)', (genesis_hash,))
            c.execute('CREATE TABLE IF NOT EXISTS txs (hash TEXT PRIMARY KEY, raw BLOB NOT NULL)')

    def connect(self):
        return sqlite3.connect(self.db, timeout=5)

    def rpc(self, method, params):
        body = json.dumps(dict(jsonrpc='2.0', id=1, method=method, params=params)).encode()
        try:
            request = urllib.request.Request(self.rpc_url, body, {'Content-Type': 'application/json'})
            with urllib.request.urlopen(request, timeout=5) as response:
                data = json.load(response)
            if 'error' in data:
                raise Unavailable('RPC error')
            return data['result']
        except (OSError, ValueError, KeyError) as e:
            raise Unavailable('RPC unavailable') from e

    def query(self, name, data=b'', height='0'):
        result = self.rpc('abci_query', dict(path='/nus.exchange.v1.Query/' + name,
                          data=data.hex(), height=height, prove=False))['response']
        h = str(result['height'])
        if integer(h) <= 0 or (height != '0' and h != height):
            raise Unavailable('inconsistent query height')
        if int(result.get('code', 0)) != 0:
            if name == 'Receipt' and re.search(r'NOT_FOUND_AT_HEIGHT ' + h + r'\b', result.get('log', '')):
                return None, h
            raise Unavailable('ABCI query failed')
        return decode_json_response(base64.b64decode(result['value'], validate=True)), h

    def snapshot(self):
        start = time.monotonic()
        s, height = self.query('Snapshot')
        if s['genesis_hash'] != self.genesis_hash or s['chain_id'] != 'nus-s1-dev-1' or s['observed_height'] != height or s['state'] != 'COMMITTED':
            raise Unavailable('network/height mismatch')
        accounts = s['accounts']
        if len(accounts) != 2 or len({a['owner'] for a in accounts}) != 2:
            raise Unavailable('invalid account set')
        for a in accounts:
            for key in ('account_number', 'sequence', 'epoch', 'bank_atoms', 'exchange_atoms', 'gas_atoms'):
                integer(a[key])
        if sum(integer(a['exchange_atoms']) for a in accounts) != integer(s['module_atoms']):
            raise Unavailable('exchange reconciliation failed')
        if sum(integer(a['bank_atoms']) for a in accounts) + integer(s['module_atoms']) != integer(s['quote_supply']) or s['quote_supply'] != '2000000000000':
            raise Unavailable('quote reconciliation failed')
        if sum(integer(a['gas_atoms']) for a in accounts + s['operator_accounts']) + integer(s['gas_collector_atoms']) != integer(s['gas_supply']) or s['gas_supply'] != s['genesis_gas_supply']:
            raise Unavailable('gas reconciliation failed')
        block = self.rpc('block', {'height': height})['block']['header']
        if str(block['height']) != height or block['chain_id'] != s['chain_id']:
            raise Unavailable('block context mismatch')
        timestamp = datetime.datetime.fromisoformat(block['time'].replace('Z', '+00:00'))
        s['block_time'] = block['time']
        s['freshness_ms'] = str(max(0, round((time.time() - timestamp.timestamp()) * 1000)))
        # No balance cache: cursor is rebuilt from committed storage on each read.
        s['query_latency_ms'] = str(round((time.monotonic() - start) * 1000))
        s['cursor_height'] = height
        s['indexer_mode'] = 'DIRECT_COMMITTED_QUERY'
        return s

    def handle(self, method, path, body=None):
        if method == 'POST' and path == '/s1/txs':
            if not isinstance(body, dict) or set(body) != {'tx_bytes'} or not isinstance(body['tx_bytes'], str):
                return 400, error('NON_CANONICAL_INPUT', False)
            try:
                raw = base64.b64decode(body['tx_bytes'], validate=True)
            except ValueError:
                return 400, error('NON_CANONICAL_INPUT', False)
            if not raw or len(raw) > 16384 or base64.b64encode(raw).decode() != body['tx_bytes']:
                return 400, error('NON_CANONICAL_INPUT', False)
            digest = hashlib.sha256(raw).hexdigest().upper()
            # Persist exact bytes before any external effect; never sign or mutate them.
            with self.connect() as c:
                c.execute('INSERT OR IGNORE INTO txs VALUES (?,?)', (digest, raw))
            out = dict(tx_hash=digest, state='SUBMISSION_UNKNOWN', check_tx_code=None, observed_height=None)
            try:
                out['observed_height'] = self.snapshot()['observed_height']
                result = self.rpc('broadcast_tx_sync', {'tx': body['tx_bytes']})
                if result['hash'].upper() != digest:
                    raise Unavailable('broadcast hash mismatch')
                out['check_tx_code'] = str(result.get('code', 0))
                out['codespace'] = result.get('codespace', '')
            except (Unavailable, KeyError, TypeError, ValueError, OSError):
                pass
            return 202, out
        if method != 'GET':
            return 405, error('METHOD_NOT_ALLOWED', False)
        s = self.snapshot()
        height = s['observed_height']
        meta = {k: s[k] for k in ('observed_height', 'cursor_height', 'query_latency_ms', 'indexer_mode', 'block_time', 'freshness_ms')}
        if path == '/s1/network':
            return 200, dict(meta, chain_id=s['chain_id'], genesis_hash=self.genesis_hash,
                             contract_version='s1-dev-1', app_version='s1-dev-1', denom='DEVQUOTE', decimals='6', gas_denom='DEVGAS')
        match = re.fullmatch(r'/s1/accounts/([^/]+)(?:/requests/([0-9a-f]{64}))?', path)
        if match:
            owner, request_id = match.groups()
            account = next((a for a in s['accounts'] if a['owner'] == owner), None)
            if account is None:
                return 404, error('UNKNOWN_ACCOUNT', False, height)
            if request_id:
                receipt, _ = self.query('Receipt', field(1, owner) + field(2, request_id), height)
                if receipt is None:
                    return 404, dict(error('NOT_FOUND_AT_HEIGHT', True, height), state='NOT_FOUND_AT_HEIGHT')
                if receipt['genesis_hash'] != self.genesis_hash or receipt['owner'] != owner or receipt['request_id'] != request_id or integer(receipt['committed_height']) > integer(height):
                    raise Unavailable('receipt context mismatch')
                return 200, receipt
            result = dict(account, **meta, state='COMMITTED', public_key_type='/cosmos.crypto.mldsa65.PubKey')
            result['public_key_base64'] = result.pop('public_key')
            return 200, result
        match = re.fullmatch(r'/s1/txs/([0-9A-F]{64})', path)
        if match:
            digest = match[1]
            unknown = dict(meta, tx_hash=digest, state='SUBMISSION_UNKNOWN', height=None, code=None, codespace=None, gas_wanted=None, gas_used=None)
            try:
                tx = self.rpc('tx', dict(hash=base64.b64encode(bytes.fromhex(digest)).decode(), prove=False))
            except Unavailable:
                return 200, unknown
            raw = base64.b64decode(tx['tx'], validate=True)
            if hashlib.sha256(raw).hexdigest().upper() != digest or tx['hash'].upper() != digest:
                raise Unavailable('indexed TX hash mismatch')
            included = str(tx['height'])
            if integer(included) == 0 or integer(included) > integer(height):
                return 200, unknown
            block = self.rpc('block', {'height': included})['block']
            index = int(tx['index'])
            if str(block['header']['height']) != included or block['header']['chain_id'] != s['chain_id'] or index < 0 or index >= len(block['data']['txs']) or block['data']['txs'][index] != tx['tx']:
                raise Unavailable('indexed TX missing from block')
            results = self.rpc('block_results', {'height': included})
            if str(results['height']) != included or index >= len(results['txs_results']):
                raise Unavailable('missing block result')
            result = results['txs_results'][index]
            if any(str(result.get(k, '')) != str(tx['tx_result'].get(k, '')) for k in ('code', 'codespace', 'gas_wanted', 'gas_used')):
                raise Unavailable('index/block result mismatch')
            code = str(result.get('code', 0))
            integer(code)
            return 200, dict(meta, tx_hash=digest, state='COMMITTED' if code == '0' else 'REJECTED_FINAL',
                             height=included, code=code, codespace=result.get('codespace', ''),
                             gas_wanted=str(result['gas_wanted']), gas_used=str(result['gas_used']))
        return 404, error('NOT_FOUND', False, height)


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def error(code, retryable, height=None):
    return dict(code=code, retryable=retryable, state='SUBMISSION_UNKNOWN', observed_height=height)


def serve(gateway, host, port, origin=None):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass  # Never log signed bytes or user input.

        def do_OPTIONS(self):
            self.respond(204, {})

        def do_GET(self):
            self.dispatch('GET')

        def do_POST(self):
            self.dispatch('POST')

        def respond(self, status, data):
            raw = json.dumps(data).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(raw)))
            self.send_header('Cache-Control', 'no-store')
            if origin and self.headers.get('Origin') == origin:
                self.send_header('Access-Control-Allow-Origin', origin)
                self.send_header('Vary', 'Origin')
                self.send_header('Access-Control-Allow-Methods', 'GET, POST, OPTIONS')
                self.send_header('Access-Control-Allow-Headers', 'Content-Type')
            self.end_headers()
            self.wfile.write(raw)

        def dispatch(self, method):
            if self.headers.get('Host') not in {f'127.0.0.1:{self.server.server_port}', f'localhost:{self.server.server_port}'}:
                return self.respond(403, error('HOST_DENIED', False))
            if self.headers.get('Origin') and self.headers['Origin'] != origin:
                return self.respond(403, error('ORIGIN_DENIED', False))
            try:
                body = None
                if method == 'POST':
                    length = int(self.headers.get('Content-Length', '0'))
                    if length <= 0 or length > 24000 or self.headers.get('Content-Type') != 'application/json':
                        return self.respond(400, error('NON_CANONICAL_INPUT', False))
                    self.connection.settimeout(5)
                    body = json.loads(self.rfile.read(length), object_pairs_hook=strict_object)
                status, data = gateway.handle(method, self.path, body)
            except (ValueError, TypeError):
                status, data = 400, error('NON_CANONICAL_INPUT', False)
            except (Unavailable, KeyError, IndexError, sqlite3.Error, OSError):
                status, data = 503, error('SNAPSHOT_UNAVAILABLE', True)
            self.respond(status, data)
    return ThreadingHTTPServer((host, port), Handler)


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--rpc', required=True)
    p.add_argument('--genesis-hash', required=True)
    p.add_argument('--journal', required=True)
    p.add_argument('--port', type=int, default=8787)
    p.add_argument('--origin')
    args = p.parse_args()
    serve(Gateway(args.rpc, args.genesis_hash, args.journal), '127.0.0.1', args.port, args.origin).serve_forever()
