"""S1 DIRECT TX routes on a pinned S2 chain, independent of matching admission."""
import base64
import hashlib
from pathlib import Path
import re
import time

from chain import Collector, durable_file, integer, json_response
from transport import Unavailable, decode, encode, unknown

RECEIPT_PATH = re.compile(r'/s1/accounts/([a-z0-9]{1,90})/requests/([0-9a-f]{64})')

ACCOUNT_PATH = re.compile(r'/s2/accounts/(nus1[a-z0-9]{1,86})')

TX_PATH = re.compile(r'/s1/txs/([0-9A-F]{64})')


def route(path):
    return (ACCOUNT_PATH.fullmatch(path) is not None or path == '/s1/txs' or TX_PATH.fullmatch(path) is not None
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

    def snapshot(self, include_body=False):
        start = time.monotonic_ns()
        status, _ = self.rpc.call('status', {})
        height = status['sync_info']['latest_block_height']
        if (status['node_info']['network'] != self.manifest['context']['chain_id']
                or status['sync_info']['catching_up'] is not False or integer(height) == 0):
            raise Unavailable('RPC_NOT_READY')
        body = self.collector.fetch(integer(height))['body']
        meta = {'observed_height': height, 'cursor_height': height,
                'query_latency_ms': str((time.monotonic_ns() - start) // 1_000_000),
                'block_time_unix_ms': body['block_time_unix_ms'],
                'freshness_ms': str(max(0, time.time_ns() // 1_000_000 - integer(body['block_time_unix_ms']))),
                'indexer_mode': 'DIRECT_COMMITTED_QUERY'}
        return (meta, body) if include_body else meta

    def account(self, owner):
        """Public committed chain state only; never expose engine R/D/P or sessions."""
        try:
            if owner not in {chain_owner(value) for value in self.manifest['owners']}:
                return 404, dict(unknown('UNKNOWN_ACCOUNT'), retryable=False)
            started = time.monotonic_ns()
            meta, body = self.snapshot(include_body=True)
            # Read the pinned profile, not client-provided freshness settings.
            profile_raw = (Path(__file__).resolve().parents[2] / 'protocol/s2/profile.json').read_bytes()
            if hashlib.sha256(profile_raw).hexdigest() != self.manifest['context']['config_hash']:
                raise Unavailable('PROFILE_HASH_MISMATCH')
            profile = decode(profile_raw)
            accounts = body['accounts']
            denoms = ['DEVBASE', 'DEVQUOTE']
            for account in accounts:
                public = base64.b64decode(account['public_key'], validate=True)
                if (len(public) != 1952 or base64.b64encode(public).decode() != account['public_key']
                        or account['public_key_type'] != 'ML_DSA_65'
                        or base64.b64encode(hashlib.sha256(public).digest()[:20]).decode() != account['owner']):
                    raise Unavailable('REGISTERED_KEY_BINDING')
                for key in ('account_number', 'sequence', 'owner_epoch', 'gas_atoms'):
                    integer(account[key])
                if [b['denom'] for b in account['balances']] != denoms:
                    raise Unavailable('ASSET_BINDING')
                for balance in account['balances']:
                    integer(balance['bank_atoms'])
                    integer(balance['confirmed_atoms'])
            if len({a['account_number'] for a in accounts}) != len(accounts):
                raise Unavailable('ACCOUNT_NUMBER_BINDING')
            if [s['denom'] for s in body['supplies']] != denoms:
                raise Unavailable('SUPPLY_BINDING')
            for i, supply in enumerate(body['supplies']):
                confirmed = sum(integer(a['balances'][i]['confirmed_atoms']) for a in accounts)
                bank = sum(integer(a['balances'][i]['bank_atoms']) for a in accounts)
                if (confirmed != integer(supply['module_atoms'])
                        or bank + confirmed != integer(supply['bank_supply_atoms'])
                        or bank + confirmed != integer(supply['genesis_supply_atoms'])):
                    raise Unavailable('ASSET_CONSERVATION')
            account = next(a for a in accounts if chain_owner(a['owner']) == owner)
            # A second status sample closes RPC loss/catchup/height changes during query.
            tip, _ = self.rpc.call('status', {})
            age = time.time_ns() // 1_000_000 - integer(body['block_time_unix_ms'])
            latency = (time.monotonic_ns() - started) // 1_000_000
            if (tip['node_info']['network'] != self.manifest['context']['chain_id']
                    or tip['sync_info']['catching_up'] is not False
                    or tip['sync_info']['latest_block_height'] != meta['observed_height']
                    or age > integer(profile['max_freshness_ms'])
                    or age < -integer(profile['max_future_block_time_ms'])
                    or latency > integer(profile['rpc_timeout_ms'])):
                raise Unavailable('ACCOUNT_NOT_FRESH')
            meta.update(freshness_ms=str(max(0, age)), query_latency_ms=str(latency))
            return 200, dict(meta, context=body['context'], state='COMMITTED',
                signing_ready=True, block_hash=body['block_hash'], owner=owner,
                owner_base64=account['owner'], public_key_type='/cosmos.crypto.mldsa65.PubKey',
                public_key_base64=account['public_key'], account_number=account['account_number'],
                sequence=account['sequence'], owner_epoch=account['owner_epoch'],
                balances=account['balances'], gas_denom=profile['gas_denom'], gas_atoms=account['gas_atoms'])
        except (Unavailable, OSError, ValueError, TypeError, KeyError, StopIteration, IndexError):
            return 503, dict(unknown(), signing_ready=False)

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
        account = ACCOUNT_PATH.fullmatch(path)
        if account and method == 'GET':
            return self.account(account[1])
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
