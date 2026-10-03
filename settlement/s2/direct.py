"""S1 DIRECT TX routes on a pinned S2 chain, independent of matching admission."""
import base64
import hashlib
from pathlib import Path
import re
import time

from chain import Collector, durable_file, integer, json_response
from transport import Unavailable, decode, encode, unknown

RECEIPT_PATH = re.compile(r'/s1/accounts/([a-z0-9]{1,90})/requests/([0-9a-f]{64})')

TX_PATH = re.compile(r'/s1/txs/([0-9A-F]{64})')


def route(path):
    return (path == '/s1/txs' or TX_PATH.fullmatch(path) is not None
            or RECEIPT_PATH.fullmatch(path) is not None)


def chain_owner(owner):
    """Canonical nus bech32 form of the pinned 20-byte S2 owner."""
    raw = base64.b64decode(owner, validate=True)
    if len(raw) != 20 or base64.b64encode(raw).decode() != owner:
        raise Unavailable('OWNER_ENCODING')
    alphabet = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l'
    value = int.from_bytes(raw, 'big')
    data = [(value >> shift) & 31 for shift in range(155, -1, -5)]
    hrp = 'nus'
    checksum = 1
    generators = (0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3)
    for item in [ord(c) >> 5 for c in hrp] + [0] + [ord(c) & 31 for c in hrp] + data + [0]*6:
        top = checksum >> 25
        checksum = ((checksum & 0x1ffffff) << 5) ^ item
        for bit, generator in enumerate(generators):
            if (top >> bit) & 1:
                checksum ^= generator
    checksum ^= 1
    return hrp + '1' + ''.join(alphabet[v] for v in data +
                              [(checksum >> shift) & 31 for shift in range(25, -1, -5)])


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

    def receipt(self, owner, request_id):
        """Public chain receipt, separate from private S2 command/session data."""
        try:
            meta = self.snapshot()
            height = meta['observed_height']
            if owner not in {chain_owner(value) for value in self.manifest['owners']}:
                return 404, dict(unknown('UNKNOWN_ACCOUNT'), retryable=False,
                                 observed_height=height)
            # Both bounded ASCII fields fit one-byte protobuf lengths.
            data = bytes((10, len(owner))) + owner.encode() + bytes((18, 64)) + request_id.encode()
            query, raw = self.rpc.call('abci_query', {
                'path': '/nus.exchange.v1.Query/Receipt', 'data': data.hex(),
                'height': height, 'prove': False})
            response = query['response']
            if response['height'] != height:
                raise Unavailable('RECEIPT_HEIGHT_MISMATCH')
            if response.get('code', 0) != 0:
                if re.search(r'NOT_FOUND_AT_HEIGHT ' + height + r'\b', response.get('log', '')):
                    return 404, dict(meta, state='NOT_FOUND_AT_HEIGHT',
                                     code='NOT_FOUND_AT_HEIGHT', retryable=True)
                raise Unavailable('RECEIPT_QUERY_FAILED')
            receipt = json_response(base64.b64decode(response['value'], validate=True))
            context = self.manifest['context']
            if (receipt['chain_id'] != context['chain_id']
                    or receipt['genesis_hash'] != context['genesis_hash']
                    or receipt['owner'] != owner or receipt['request_id'] != request_id
                    or not 0 < integer(receipt['committed_height']) <= integer(height)
                    or receipt['state'] != 'COMMITTED'
                    or not re.fullmatch(r'[0-9A-F]{64}', receipt['original_tx_hash'])):
                raise Unavailable('RECEIPT_BINDING')
            durable_file(self.evidence / ('receipt-' + hashlib.sha256(raw).hexdigest() + '.json'), raw)
            return 200, receipt
        except (Unavailable, OSError, ValueError, TypeError, KeyError):
            return 503, unknown()

    def request(self, method, path, raw=b''):
        receipt = RECEIPT_PATH.fullmatch(path)
        if receipt and method == 'GET':
            return self.receipt(*receipt.groups())
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
