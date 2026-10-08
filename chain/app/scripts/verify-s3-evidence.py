#!/usr/bin/env python3
"""Independent integer/hash reconciliation of captured S3 RPC bytes (stdlib)."""
import base64
import copy
import hashlib
import json
import sys
from pathlib import Path


def raw(s):
    return base64.b64decode(s, validate=True)


def sha(b):
    return hashlib.sha256(b).hexdigest()


def frame(domain, b):
    d = domain.encode('ascii')
    return len(d).to_bytes(4, 'big') + d + len(b).to_bytes(8, 'big') + b


def canonical(v):
    return json.dumps(v, sort_keys=True, ensure_ascii=True, separators=(',', ':')).encode()


def batch_core(data):
    def varint(pos):
        value, shift = 0, 0
        while True:
            byte = data[pos]
            pos += 1
            value |= (byte & 127) << shift
            if byte < 128:
                return value, pos
            shift += 7
            assert shift < 70
    result, pos = b'', 0
    while pos < len(data):
        start = pos
        key, pos = varint(pos)
        if key & 7 == 0:
            _, pos = varint(pos)
        else:
            assert key & 7 == 2
            size, pos = varint(pos)
            pos += size
        assert pos <= len(data)
        if key >> 3 != 7:
            result += data[start:pos]
    return result


def verify(evidence):
    count = 0
    def check(condition, label):
        nonlocal count
        count += 1
        assert condition, label

    e = evidence
    genesis = json.loads(raw(e['raw_genesis']))
    check(sha(raw(e['raw_genesis'])) == e['genesis_hash'], 'exact runtime genesis')
    check(genesis['chain_id'] == 'nus-s3-dev-1', 'chain id')
    check(len(genesis['validators']) == 4, 'four validators')
    check(all(int(v['power']) == 10 for v in genesis['validators']), 'validator power')
    check(int(genesis['consensus_params']['block']['max_gas']) == 20_000_000, 'block gas')
    txs = e['txs']
    check(len(txs) == 6, 'two deposits, settlement, retry, two withdrawals')
    blocks = {}
    for tx in txs:
        check(sha(raw(tx['raw_tx'])) == tx['tx_hash'], 'raw TX hash')
        block = json.loads(raw(tx['raw_block']))['result']
        results = json.loads(raw(tx['raw_block_results']))['result']
        height, index = int(tx['height']), int(tx['tx_index'])
        check(int(block['block']['header']['height']) == height == int(results['height']), 'confirmed height')
        check(block['block']['header']['chain_id'] == genesis['chain_id'], 'block chain id')
        check(raw(block['block']['data']['txs'][index]) == raw(tx['raw_tx']), 'inclusion index/bytes')
        result = results['txs_results'][index]
        check(int(result.get('code', 0)) == 0, 'finalized code')
        check(int(result['gas_used']) == int(tx['gas_used']) <= 10_000_000, 'actual gas')
        blocks[height] = block
    receipt = e['receipt']['receipt']
    batch = raw(e['batch_wire'])
    check(sha(frame('NUS/BATCH_HASH/V2', batch)) == receipt['batch']['batch_hash'], 'batch hash')
    check(sha(frame('NUS/BATCH_ID/V2', batch_core(batch))) == receipt['batch']['batch_id'], 'batch id')
    check(receipt['batch']['batch_seq'] == '1' and receipt['disposition'] == 'COMMITTED', 'slot identity')
    check(receipt['terminal_tx_hash'] == txs[2]['tx_hash'], 'first successful TX')
    check(receipt['terminal_height'] == txs[2]['height'], 'first successful height')
    check(receipt['context']['genesis_hash'] == e['genesis_hash'], 'receipt context')
    check(receipt == e['restart_receipt']['receipt'], 'restart immutable receipt')
    check(e['restart_confirmed_tx']['tx_hash'] == receipt['terminal_tx_hash'], 'restart finalized TX hash')
    check(e['restart_confirmed_tx']['height'] == receipt['terminal_height'], 'restart finalized TX height')
    fee = int(e['fee_bps'])
    check(fee in (0, 25), 'profile')
    seller, buyer = [sha(raw(pk))[:40] for pk in genesis['app_state']['public_keys']]
    for stage in ('settled_snapshot', 'withdrawn_snapshot'):
        snapshot = e[stage]
        body = {k: v for k, v in snapshot.items() if k != 'snapshot_id'}
        check(sha(frame('NUS/S3/CHAIN_SNAPSHOT/V1', canonical(body))) == snapshot['snapshot_id'], 'snapshot hash')
        check(snapshot['context'] == receipt['context'], 'snapshot context')
        check(snapshot['last_batch_seq'] == '1' and snapshot['last_batch_hash'] == receipt['batch']['batch_hash'], 'retry has one slot')
        if stage == 'settled_snapshot':
            check(snapshot['height'] == receipt['terminal_height'], 'same-height C/receipt')
            check(snapshot['block_hash'] == blocks[int(snapshot['height'])]['block_id']['hash'].lower(), 'snapshot block hash')
        confirmed, bank = {'DEVBASE': 0, 'DEVQUOTE': 0}, {'DEVBASE': 0, 'DEVQUOTE': 0}
        received_base = 1_000_000 - (1_000_000 * fee + 9999) // 10000
        received_quote = 10_000_000 - (10_000_000 * fee + 9999) // 10000
        expected = {seller: {'DEVBASE': 9_000_000, 'DEVQUOTE': received_quote},
                    buyer: {'DEVBASE': received_base, 'DEVQUOTE': 90_000_000}}
        if stage == 'withdrawn_snapshot':
            expected[seller]['DEVQUOTE'] = expected[buyer]['DEVBASE'] = 0
        check(len(snapshot['accounts']) == 2, 'account set size')
        for account in snapshot['accounts']:
            owner = raw(account['owner']).hex()
            check(owner in expected, 'registered account set')
            for asset in account['assets']:
                denom, amount = asset['denom'], int(asset['confirmed_atoms'])
                check(amount == expected[owner][denom], 'exact integer balance')
                confirmed[denom] += amount
                bank[denom] += int(asset['bank_atoms'])
        for asset in snapshot['assets']:
            denom = asset['denom']
            c, tr, u, module = (int(asset[k]) for k in ('sum_confirmed_atoms', 'treasury_atoms', 'unassigned_atoms', 'module_bank_atoms'))
            check(c == confirmed[denom] and min(c, tr, u, module) >= 0, 'nonnegative C sum')
            check(module == c + tr + u, 'module=C+T+U')
            check(bank[denom] + module == int(asset['supply_atoms']) == 2_000_000_000_000, 'global asset conservation')
            expected_fee = (1_000_000 if denom == 'DEVBASE' else 10_000_000) * fee // 10000
            check(tr == expected_fee and u == 0, 'treasury/unassigned profile')
    return count


def main():
    root = Path(sys.argv[1])
    files = sorted(root.glob('TestS3CometLocal*/comet-evidence.json'))
    assert len(files) == 6, f'expected six fresh network runs, got {len(files)}'
    runs = []
    for path in files:
        e = json.loads(path.read_text())
        checks = verify(e)
        mutations = [
            ('genesis', lambda x: x.update(genesis_hash='0' * 64)),
            ('raw_tx', lambda x: x['txs'][2].update(tx_hash='0' * 64)),
            ('inclusion', lambda x: x['txs'][2].update(tx_index='999')),
            ('batch', lambda x: x['receipt']['receipt']['batch'].update(batch_hash='0' * 64)),
            ('receipt', lambda x: x['receipt']['receipt'].update(terminal_height='999')),
            ('balance', lambda x: x['settled_snapshot']['accounts'][0]['assets'][0].update(confirmed_atoms='1')),
            ('treasury', lambda x: x['settled_snapshot']['assets'][0].update(treasury_atoms='1')),
        ]
        detected = []
        for name, mutate in mutations:
            bad = copy.deepcopy(e)
            mutate(bad)
            try:
                verify(bad)
            except (AssertionError, IndexError):
                detected.append(name)
            else:
                raise AssertionError(f'mutant was not detected: {name}')
        runs.append({'file': str(path.relative_to(root)), 'sha256': sha(path.read_bytes()),
                     'genesis_hash': e['genesis_hash'], 'checks': checks, 'mutants_detected': detected,
                     'expected_diff': [], 'result': 'PASS'})
    print(json.dumps({'result': 'PASS', 'runs': runs, 'checks': sum(x['checks'] for x in runs),
                      'mutants_detected': sum(len(x['mutants_detected']) for x in runs)}, indent=2))


if __name__ == '__main__':
    main()
