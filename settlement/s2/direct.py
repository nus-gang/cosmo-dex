"""S1 DIRECT TX routes on a pinned S2 chain, independent of matching admission."""
import base64
import hashlib
from pathlib import Path
import re
import time

from chain import Collector, durable_file, integer
from transport import Unavailable, decode, encode, unknown

TX_PATH = re.compile(r'/s1/txs/([0-9A-F]{64})')


def route(path):
    return path == '/s1/txs' or TX_PATH.fullmatch(path) is not None


class Direct:
    def __init__(self, rpc, manifest, evidence):
        self.rpc, self.manifest = rpc, manifest
        self.evidence = Path(evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        durable_file(self.evidence / 'context.json', encode(manifest['context']))
        self.collector = Collector(rpc, None, manifest, self.evidence / 'observations')

    def snapshot(self):
        start = time.monotonic_ns()
        status, _ = self.rpc.call('status', {})
        height = status['sync_info']['latest_block_height']
        if (status['node_info']['network'] != self.manifest['context']['chain_id']
                or status['sync_info']['catching_up'] is not False or integer(height) == 0):
            raise Unavailable('RPC_NOT_READY')
        body = self.collector.fetch(integer(height))['body']
        return {'observed_height': height, 'cursor_height': height,
                'query_latency_ms': str((time.monotonic_ns() - start) // 1_000_000),
                'block_time_unix_ms': body['block_time_unix_ms'],
                'freshness_ms': str(max(0, time.time_ns() // 1_000_000 - integer(body['block_time_unix_ms']))),
                'indexer_mode': 'DIRECT_COMMITTED_QUERY'}

    def request(self, method, path, raw=b''):
        if method == 'POST' and path == '/s1/txs':
            try:
                body = decode(raw)
                if not isinstance(body, dict) or set(body) != {'tx_bytes'}:
                    raise ValueError('shape')
                tx = base64.b64decode(body['tx_bytes'], validate=True)
                if not tx or len(tx) > 16384 or base64.b64encode(tx).decode() != body['tx_bytes']:
                    raise ValueError('bytes')
            except (ValueError, TypeError, KeyError):
                return 400, dict(unknown('NON_CANONICAL_INPUT'), state='REJECTED', retryable=False)
            digest = hashlib.sha256(tx).hexdigest().upper()
            out = dict(tx_hash=digest, state='SUBMISSION_UNKNOWN', check_tx_code=None, observed_height=None)
            try:
                # Immutable exact bytes, fsynced before broadcast. No startup resend.
                durable_file(self.evidence / (digest + '.tx'), tx)
                out['observed_height'] = self.snapshot()['observed_height']
                result, _ = self.rpc.call('broadcast_tx_sync', {'tx': body['tx_bytes']})
                if result['hash'].upper() != digest:
                    raise Unavailable('TX_HASH_MISMATCH')
                code = str(result.get('code', 0))
                integer(code)
                out.update(check_tx_code=code, codespace=result.get('codespace', ''))
            except (Unavailable, OSError, ValueError, TypeError, KeyError):
                pass
            return 202, out
        match = TX_PATH.fullmatch(path)
        if method != 'GET' or not match:
            return 405, dict(unknown('METHOD_NOT_ALLOWED'), state='REJECTED', retryable=False)
        digest = match[1]
        try:
            meta = self.snapshot()
            out = dict(meta, tx_hash=digest, state='SUBMISSION_UNKNOWN', height=None,
                       code=None, codespace=None, gas_wanted=None, gas_used=None)
            try:
                tx, _ = self.rpc.call('tx', {'hash': base64.b64encode(bytes.fromhex(digest)).decode(), 'prove': False})
            except Unavailable:
                return 200, out
            raw_tx = base64.b64decode(tx['tx'], validate=True)
            if (hashlib.sha256(raw_tx).hexdigest().upper() != digest
                    or tx['hash'].upper() != digest):
                raise Unavailable('TX_HASH_MISMATCH')
            height = tx['height']
            if integer(height) == 0 or integer(height) > integer(meta['observed_height']):
                return 200, out
            block, _ = self.rpc.call('block', {'height': height})
            block = block['block']
            index = tx['index']
            if (type(index) is not int or index < 0
                    or block['header']['height'] != height
                    or block['header']['chain_id'] != self.manifest['context']['chain_id']
                    or index >= len(block['data']['txs']) or block['data']['txs'][index] != tx['tx']):
                raise Unavailable('TX_BLOCK_MISMATCH')
            results, _ = self.rpc.call('block_results', {'height': height})
            if results['height'] != height or index >= len(results['txs_results']):
                raise Unavailable('TX_RESULT_MISMATCH')
            result = results['txs_results'][index]
            if any(str(result.get(k, '')) != str(tx['tx_result'].get(k, ''))
                   for k in ('code', 'codespace', 'gas_wanted', 'gas_used')):
                raise Unavailable('TX_RESULT_MISMATCH')
            code = str(result.get('code', 0))
            integer(code)
            return 200, dict(meta, tx_hash=digest, state='COMMITTED' if code == '0' else 'REJECTED_FINAL',
                             height=height, code=code, codespace=result.get('codespace', ''),
                             gas_wanted=str(result['gas_wanted']), gas_used=str(result['gas_used']))
        except (Unavailable, OSError, ValueError, TypeError, KeyError, IndexError):
            return 503, dict(unknown(), tx_hash=digest)
