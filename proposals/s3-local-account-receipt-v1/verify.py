"""Read-only contract checks. Never run a service, sign, replay a product or rewrite vectors."""
import base64
import copy
import json
import sys
from pathlib import Path
import reference as m

checks = []


def yes(value, label):
    if not value:
        raise AssertionError(label)
    checks.append({'id': label, 'result': 'PASS'})


def rejects(fn, code, label):
    try:
        fn()
    except m.ReceiptError as e:
        yes(str(e) == code, label + '/' + str(e))
    else:
        raise AssertionError('accepted: ' + label)


def main():
    yes(not sys.flags.optimize, 'assertions-enabled-for-inherited-schema-oracle')
    manifest = json.loads((m.HERE / 'MANIFEST.json').read_text())
    for path, digest in manifest['files_sha256'].items():
        yes(m.sha((m.ROOT / path).read_bytes()) == digest, 'candidate-hash/' + path)
    for path, digest in manifest['inherited_files_sha256'].items():
        yes(m.sha((m.ROOT / path).read_bytes()) == digest, 'inherited-hash/' + path)
    yes(m.sha(''.join(f'{v}  {k}\n' for k, v in sorted(manifest['files_sha256'].items())).encode())
        == manifest['candidate_files_sha256'], 'candidate-aggregate')
    index = json.loads((m.HERE / 'vectors/index.json').read_text())
    for case in index['cases']:
        frame = (m.HERE / 'vectors' / case['source']).read_bytes()
        want = (m.HERE / 'vectors' / case['expected']).read_bytes()
        p, ctx = case['principal'], case['context']
        yes(m.sha(frame) == case['source_sha256'] and m.sha(want) == case['expected_sha256'], case['id'] + '/golden-bytes')
        got = m.project(frame, p, ctx)
        yes(got == want and str(len(got)) == case['expected_bytes'], case['id'] + '/projection')
        m.verify_trusted(want, frame, p, ctx)
        rec, original, state = m.source(frame)
        v = m.validate_public(got, p, ctx)
        yes(v['source'] == case['source_tuple'], case['id'] + '/tuple')
        for row in state['orders']:
            if row['owner'] != p:
                yes(row['view']['order_hash'].encode() not in got, case['id'] + '/no-foreign-order/' + row['view']['order_hash'])
        for a in state['accounts']:
            if a['owner'] != p:
                yes(a['owner'].encode() not in got, case['id'] + '/no-foreign-owner/' + a['owner'])
        # Deserialize raw immutable bytes again. This is model re-derivation,
        # explicitly NOT a real same-home restart/semantic replay test.
        for replay in range(2):
            fresh_frame = base64.b64decode(base64.b64encode(frame))
            again = m.project(fresh_frame, p, copy.deepcopy(ctx))
            m.reconcile(got, again, p, ctx)
            yes(again == got, case['id'] + '/model-reread-' + str(replay + 1))
        yes('command_result' not in v and 'correction_results' not in v['account_result'], case['id'] + '/separate-type')

    by_id = {c['id']: c for c in index['cases']}
    for fee, net in [('fee0', 1000000), ('fee25', 997500)]:
        c = by_id[fee + '-taker-trade']
        v = json.loads((m.HERE / 'vectors' / c['expected']).read_bytes())
        rows = {x['after']['denom']: x['after'] for x in v['account_result']['ledger_changes']}
        yes(rows['DEVBASE']['P'] == str(net) and rows['DEVBASE']['A'] == '0', fee + '/net-P-not-spendable')
        yes(rows['DEVQUOTE']['D'] == '12000000' and rows['DEVQUOTE']['A'] == '88000000', fee + '/gross-D-price-improvement-held')
        yes(len(v['account_result']['affected_order_hashes']) == 1 and len(v['account_result']['created_fill_ids']) == 1, fee + '/own-ID-count')

    c = by_id['fee0-taker-trade']
    raw = (m.HERE / 'vectors' / c['expected']).read_bytes()
    frame = (m.HERE / 'vectors' / c['source']).read_bytes()
    value, p, ctx = json.loads(raw), c['principal'], c['context']
    record, result, state = m.source(frame)
    m.validate_request_receipt(raw, p, ctx, 'ORDER', result['request_hash'], value['source'])
    yes(True, 'request-binding-exact')
    rejects(lambda: m.validate_request_receipt(raw, p, ctx, 'CANCEL', result['request_hash']),
            'CLIENT_RECEIPT_MISMATCH', 'request-kind-mismatch')
    rejects(lambda: m.validate_request_receipt(raw, p, ctx, 'ORDER', '0' * 64),
            'CLIENT_RECEIPT_MISMATCH', 'request-hash-mismatch')
    wrong_seq = copy.deepcopy(value['source']); wrong_seq['command_seq'] = '3'
    rejects(lambda: m.validate_request_receipt(raw, p, ctx, 'ORDER', result['request_hash'], wrong_seq),
            'CLIENT_RECEIPT_MISMATCH', 'same-request-new-source')
    # Negative golden files have fixed raw bytes, expected error and hash.
    negatives = json.loads((m.HERE / 'vectors/negative-index.json').read_text())
    for n in negatives:
        b = (m.HERE / 'vectors' / n['file']).read_bytes()
        yes(m.sha(b) == n['sha256'], 'negative-bytes/' + n['id'])
        rejects(lambda b=b: m.validate_public(b, p, ctx), n['error'], n['id'])
    for key in list(value):
        changed = copy.deepcopy(value); del changed[key]
        rejects(lambda: m.validate_public(m.canon(changed), p, ctx), 'RECEIPT_SCHEMA', 'missing/' + key)
    for key in value['source']:
        changed = copy.deepcopy(value); del changed['source'][key]
        rejects(lambda: m.validate_public(m.canon(changed), p, ctx), 'RECEIPT_SCHEMA', 'missing-source/' + key)
    for key in value['account_result']:
        changed = copy.deepcopy(value); del changed['account_result'][key]
        rejects(lambda: m.validate_public(m.canon(changed), p, ctx), 'RECEIPT_SCHEMA', 'missing-result/' + key)
    # A client can accept a fresh schema-valid lie; it cannot prove hidden source.
    changed = copy.deepcopy(value); changed['source']['record_hash'] = '0' * 64
    lie = m.canon(changed)
    m.validate_public(lie, p, ctx)
    rejects(lambda: m.reconcile(raw, lie, p, ctx), 'CLIENT_RECEIPT_MISMATCH', 'client-requery-difference')
    rejects(lambda: m.verify_trusted(lie, frame, p, ctx), 'SOURCE_PROJECTION_MISMATCH', 'trusted-rejects-source-lie')
    changed = copy.deepcopy(value); changed['account_result']['created_fill_ids'] = []
    rejects(lambda: m.verify_trusted(m.canon(changed), frame, p, ctx), 'SOURCE_PROJECTION_MISMATCH', 'trusted-rejects-omission')
    changed = copy.deepcopy(value); changed['account_result']['affected_order_hashes'] = result['affected_order_hashes']
    rejects(lambda: m.verify_trusted(m.canon(changed), frame, p, ctx), 'SOURCE_PROJECTION_MISMATCH', 'trusted-rejects-foreign-ID')
    other = next(a['owner'] for a in state['accounts'] if a['owner'] != p)
    rejects(lambda: m.project(frame, other, ctx), 'RECEIPT_NOT_FOUND', 'maker-cannot-query-taker-command')
    rejects(lambda: m.project(b'', p, ctx), 'SOURCE_INVALID', 'missing-source')
    rejects(lambda: m.project(frame[:-1], p, ctx), 'SOURCE_INVALID', 'truncated-source')
    bad_frame = bytearray(frame); bad_frame[40] ^= 1
    rejects(lambda: m.project(bytes(bad_frame), p, ctx), 'SOURCE_INVALID', 'frame-checksum')
    for key in ['result_hash', 'after_state_hash', 'command_seq']:
        r = copy.deepcopy(record); r[key] = '0' * 64 if key.endswith('hash') else '9'
        rejects(lambda: m.project(m.frame(r), p, ctx), 'SOURCE_INVALID', 'rehashed-record-tuple/' + key)
    for name in ['record_hash', 'command_result_hash', 'after_state_hash']:
        changed = copy.deepcopy(value); changed['source'][name] = '0' * 64
        rejects(lambda: m.verify_trusted(m.canon(changed), frame, p, ctx), 'SOURCE_PROJECTION_MISMATCH', 'public-tuple/' + name)
    # An unrelated internal command cannot become a public activity oracle.
    unrelated = copy.deepcopy(record); rr = copy.deepcopy(result)
    rr['kind'] = unrelated['command_kind'] = 'SNAPSHOT'
    for name in m.LISTS: rr[name] = []
    rr['ledger_changes'] = []
    unrelated['result_hash'] = m.domain_hash('NUS/S3/COMMAND_RESULT/V1', rr)
    unrelated['result_json'] = base64.b64encode(m.canon(rr)).decode()
    rejects(lambda: m.project(m.frame(unrelated), p, ctx), 'RECEIPT_NOT_FOUND', 'unrelated-internal-command')
    r = copy.deepcopy(record); del r['state_json']
    rejects(lambda: m.project(m.frame(r), p, ctx), 'SOURCE_INVALID', 'missing-immutable-state')
    later = copy.deepcopy(record)
    s = copy.deepcopy(state); s['last_command_seq'] = '999'
    later['state_json'] = base64.b64encode(m.canon(s)).decode()
    rejects(lambda: m.project(m.frame(later), p, ctx), 'SOURCE_INVALID', 'later-state-in-old-receipt')

    # Explicit integer boundaries, no JS Number round trip.
    for n in ['9007199254740993', '18446744073709551615']:
        v = copy.deepcopy(value); v['source']['command_seq'] = n
        yes(m.validate_public(m.canon(v), p, ctx)['source']['command_seq'] == n, 'u64/' + n)
    v = copy.deepcopy(value)
    row = v['account_result']['ledger_changes'][0]
    for side in ('before', 'after'):
        row[side].update(C=str(2**128 - 1), A=str(2**128 - 1), R='0', D='0', P=str(2**128 - 1))
    m.validate_public(m.canon(v), p, ctx); yes(True, 'u128-maximum')
    # Array bound is structural; public clients cannot infer ownership of novel IDs.
    v = copy.deepcopy(value); v['account_result']['corrected_fill_ids'] = [f'{i:064x}' for i in range(1001)]
    m.validate_public(m.canon(v), p, ctx); yes(True, '1001-not-truncated-to-page-cap')
    bound = m.SCHEMA['$defs']['AccountResult']['properties']['corrected_fill_ids']['maxItems']
    spec = m.SCHEMA['$defs']['AccountResult']['properties']['corrected_fill_ids']
    m.schema(['0' * 64] * bound, spec, m.SCHEMA['$defs']); yes(True, 'array-structural-cap')
    rejects(lambda: m.schema(['0' * 64] * (bound + 1), spec, m.SCHEMA['$defs']), 'RECEIPT_SCHEMA', 'array-cap-plus-one')
    rejects(lambda: m.validate_public(b' ' * (m.CAP + 1), p, ctx), 'RECEIPT_LIMIT', 'body-cap-plus-one')
    rejects(lambda: m.validate_public(b'[' * 9 + b']' * 9, p, ctx), 'RECEIPT_LIMIT', 'depth-cap-plus-one')
    # Conservative new-field overhead: max U64/context hash lengths and all lists empty.
    v = copy.deepcopy(value)
    for key in m.LISTS: v['account_result'][key] = []
    v['account_result']['ledger_changes'] = []
    v['source']['command_seq'] = str(2**64 - 1)
    v['context']['market_config_version'] = str(2**64 - 1)
    v['account_result']['observed_height'] = str(2**64 - 1)
    v['account_result']['kind'] = 'WITHDRAW_PREPARE'
    v['account_result']['code'] = max(m.SCHEMA['$defs']['Code']['enum'], key=len)
    yes(len(m.canon(v)) < 4096, 'envelope-overhead-under-4096')
    yes(m.CAP * 3 // 4 + 4096 < m.CAP and bound == (m.CAP - 1) // 67, 'cap-derivation')
    print(json.dumps({'scope': 'AUTHOR_STATIC_CONTRACT_ORACLE_ONLY', 'checks': len(checks),
                      'PASS': len(checks), 'FAIL': 0, 'golden_positive': len(index['cases']),
                      'golden_negative': len(negatives), 'product_acceptance': 'NOT_RUN',
                      'G00': 'FAIL_UNPROVEN', 'ACK': 'CLOSED', 'checks_detail': checks}, indent=2))


if __name__ == '__main__':
    main()
