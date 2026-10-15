"""CTO contract oracle, NOT a server, signature verifier or economic replay engine.

Only call project after the rc3 trusted adapter has checked semantic replay,
the complete WAL prefix/marker, evidence closure and authentication. These
requirements are not reduced to caller-supplied boolean flags by this oracle.
"""
import base64
import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
CAP = 16777216
VERSION = 's3-dev-local-account/1'
USER_KINDS = {'ORDER', 'CANCEL', 'WITHDRAW_PREPARE', 'WITHDRAW_ABORT'}
LISTS = ('affected_order_hashes', 'created_fill_ids', 'corrected_fill_ids',
         'committed_fill_ids', 'applied_batch_ids')
SCHEMA = json.loads((HERE / 'schema.json').read_text())
sys.path.insert(0, str(ROOT / 'protocol/s3/tools'))
import check as inherited


class ReceiptError(ValueError):
    pass


def require(ok, code):
    if not ok:
        raise ReceiptError(code)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def canon(v):
    def values(x):
        require(not isinstance(x, (int, float)) or isinstance(x, bool), 'RECEIPT_CANONICAL')
        if isinstance(x, str):
            require(not any(0xd800 <= ord(c) <= 0xdfff for c in x), 'RECEIPT_CANONICAL')
        if isinstance(x, dict):
            for k, y in x.items():
                require(k.isascii(), 'RECEIPT_CANONICAL')
                values(y)
        if isinstance(x, list):
            for y in x:
                values(y)
    values(v)
    return json.dumps(v, sort_keys=True, ensure_ascii=True, separators=(',', ':'), allow_nan=False).encode()


def decode(raw, limit=CAP):
    require(len(raw) <= limit, 'RECEIPT_LIMIT')
    def pairs(rows):
        d = {}
        for k, v in rows:
            require(k not in d, 'RECEIPT_CANONICAL')
            d[k] = v
        return d
    try:
        v = json.loads(raw.decode('utf-8'), object_pairs_hook=pairs,
                       parse_constant=lambda _: (_ for _ in ()).throw(ReceiptError('RECEIPT_CANONICAL')))
        require(canon(v) == raw, 'RECEIPT_CANONICAL')
        return v
    except (UnicodeError, json.JSONDecodeError, RecursionError) as e:
        raise ReceiptError('RECEIPT_CANONICAL') from e


def public_depth(raw):
    # Bound hostile nesting before json.loads; brackets inside strings are data.
    depth = 0
    string = escaped = False
    for c in raw:
        if string:
            if escaped:
                escaped = False
            elif c == 92:
                escaped = True
            elif c == 34:
                string = False
        elif c == 34:
            string = True
        elif c in (91, 123):
            depth += 1
            require(depth <= 8, 'RECEIPT_LIMIT')
        elif c in (93, 125):
            depth -= 1


def domain_hash(domain, v):
    d, b = domain.encode('ascii'), canon(v)
    return sha(len(d).to_bytes(4, 'big') + d + len(b).to_bytes(8, 'big') + b)


def b64(s):
    try:
        b = base64.b64decode(s, validate=True)
        require(base64.b64encode(b).decode() == s, 'RECEIPT_SCHEMA')
        return b
    except (ValueError, TypeError) as e:
        raise ReceiptError('RECEIPT_SCHEMA') from e


def schema(v, spec, defs):
    if '$ref' in spec:
        name = spec['$ref'].split('/')[-1]
        schema(v, defs[name], defs)
        if name in ('U64', 'Atoms'):
            require(int(v) < 2 ** (64 if name == 'U64' else 128), 'RECEIPT_SCHEMA')
        if name == 'Owner':
            require(len(b64(v)) == 20, 'RECEIPT_SCHEMA')
        return
    if 'const' in spec:
        require(type(v) is type(spec['const']) and v == spec['const'], 'RECEIPT_SCHEMA')
    if 'enum' in spec:
        require(v in spec['enum'], 'RECEIPT_SCHEMA')
    t = spec.get('type')
    if t == 'object':
        require(type(v) is dict and set(v) == set(spec['required']), 'RECEIPT_SCHEMA')
        for k in v:
            schema(v[k], spec['properties'][k], defs)
    elif t == 'array':
        require(type(v) is list and len(v) <= spec['maxItems'], 'RECEIPT_SCHEMA')
        for x in v:
            schema(x, spec['items'], defs)
    elif t == 'string':
        require(type(v) is str and len(v) <= spec.get('maxLength', CAP), 'RECEIPT_SCHEMA')
        if 'pattern' in spec:
            require(re.fullmatch(spec['pattern'], v) is not None, 'RECEIPT_SCHEMA')
    elif t == 'boolean':
        require(type(v) is bool, 'RECEIPT_SCHEMA')


def validate_public(raw, principal, context):
    require(len(raw) <= CAP, 'RECEIPT_LIMIT')
    public_depth(raw)
    v = decode(raw)
    schema(v, SCHEMA, SCHEMA['$defs'])
    require(v['principal'] == principal, 'RECEIPT_PRINCIPAL')
    require(v['context'] == context, 'RECEIPT_CONTEXT')
    r = v['account_result']
    require(int(v['source']['command_seq']) > 0, 'RECEIPT_SCHEMA')
    require((r['code'] == 'OK') == (r['state'] == 'LOCAL_ACCEPTED'), 'RECEIPT_SCHEMA')
    for name in LISTS:
        require(len(r[name]) == len(set(r[name])), 'RECEIPT_SCHEMA')
    denoms = []
    for row in r['ledger_changes']:
        require(row['owner'] == principal, 'RECEIPT_PRINCIPAL')
        require(row['before']['denom'] == row['after']['denom'], 'RECEIPT_SCHEMA')
        denoms.append(row['after']['denom'])
        for side in ('before', 'after'):
            x = row[side]
            require(int(x['C']) - int(x['R']) - int(x['D']) == int(x['A']), 'RECEIPT_SCHEMA')
    require(denoms == sorted(set(denoms)), 'RECEIPT_SCHEMA')
    return v


def frame(record):
    raw = canon(record)
    require(len(raw) <= CAP, 'RECEIPT_LIMIT')
    h = b'S3D1' + len(raw).to_bytes(4, 'big') + hashlib.sha256(raw).digest()
    return h + hashlib.sha256(h).digest() + raw


def source(raw_frame):
    require(72 <= len(raw_frame) <= CAP + 72, 'SOURCE_INVALID')
    h, raw = raw_frame[:72], raw_frame[72:]
    require(h[:4] == b'S3D1' and int.from_bytes(h[4:8], 'big') == len(raw)
            and hashlib.sha256(raw).digest() == h[8:40]
            and hashlib.sha256(h[:40]).digest() == h[40:], 'SOURCE_INVALID')
    record = decode(raw)
    try:
        result, state = decode(b64(record['result_json'])), decode(b64(record['state_json']))
        for name, obj in [('JournalRecord', record), ('CommandResult', result), ('EngineState', state)]:
            inherited.validate(obj, {'$ref': '#/$defs/' + name}, inherited.read('schema.json')['$defs'])
    except (AssertionError, ValueError, TypeError, KeyError) as e:
        raise ReceiptError('SOURCE_INVALID') from e
    seq = result['command_seq']
    require(seq == record['command_seq'] == state['last_command_seq'], 'SOURCE_INVALID')
    require(record['command_kind'] == result['kind'], 'SOURCE_INVALID')
    require(record['context'] == state['context'] == record['snapshot']['context'], 'SOURCE_INVALID')
    require(record['after_state_hash'] == result['after_state_hash']
            == domain_hash('NUS/S3/ENGINE_STATE/V1', state), 'SOURCE_INVALID')
    require(record['result_hash'] == domain_hash('NUS/S3/COMMAND_RESULT/V1', result), 'SOURCE_INVALID')
    require(result['snapshot_id'] == record['snapshot']['snapshot_id']
            and result['observed_height'] == record['snapshot']['height'], 'SOURCE_INVALID')
    return record, result, state


def project(raw_frame, principal, context):
    """Projection of already-authenticated, marker-covered, replay-valid source.

    Re-check byte/hash bindings here. This pure function cannot establish marker
    completion, crypto validity, session TTL or economic authority by itself.
    """
    record, result, state = source(raw_frame)
    require(record['context'] == context, 'RECEIPT_CONTEXT')
    require(len(b64(principal)) == 20, 'RECEIPT_PRINCIPAL')
    owners = [a['owner'] for a in state['accounts']]
    require(principal in owners and len(set(owners)) == len(owners), 'RECEIPT_PRINCIPAL')
    orders = {o['view']['order_hash']: o for o in state['orders']}
    fills = {f['fill_id']: f for f in state['fills']}
    require(len(orders) == len(state['orders']) and len(fills) == len(state['fills']), 'SOURCE_INVALID')
    own_orders = {h for h, o in orders.items() if o['owner'] == principal}
    own_fills, own_batches, all_batches = set(), set(), set()
    for fid, f in fills.items():
        require(f['buyer_order_hash'] in orders and f['seller_order_hash'] in orders, 'SOURCE_INVALID')
        participates = bool(own_orders.intersection([f['buyer_order_hash'], f['seller_order_hash']]))
        if participates:
            own_fills.add(fid)
        if f['batch'] is not None:
            all_batches.add(f['batch']['batch_id'])
            if participates:
                own_batches.add(f['batch']['batch_id'])
    r = {k: result[k] for k in ('kind', 'request_hash', 'code', 'state', 'observed_height', 'snapshot_id')}
    for name in LISTS:
        known, own = (orders, own_orders) if name == 'affected_order_hashes' else (
            (all_batches, own_batches) if name == 'applied_batch_ids' else (fills, own_fills))
        require(len(result[name]) == len(set(result[name])) and all(x in known for x in result[name]), 'SOURCE_INVALID')
        r[name] = [x for x in result[name] if x in own]
    r['ledger_changes'] = [x for x in result['ledger_changes'] if x['owner'] == principal]
    if result['kind'] in USER_KINDS:
        bindings = [b for b in state['bindings'] if b['kind'] == result['kind']
                    and b['first_command_seq'] == result['command_seq']
                    and b['request_hash'] == result['request_hash']]
        require(len(bindings) == 1 and bindings[0]['owner'] == principal, 'RECEIPT_NOT_FOUND')
    else:
        require(any(r[k] for k in LISTS) or r['ledger_changes'], 'RECEIPT_NOT_FOUND')
    v = {'envelope_version': VERSION, 'profile_id': 's3-dev-local-v1', 'context': context,
         'principal': principal, 'development_receipt': 'LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE',
         'durable_ack': False, 'storage_assurance': 'UNPROVEN_HOST_SPACE',
         'source': {'command_seq': result['command_seq'], 'record_hash': sha(raw_frame),
                    'command_result_hash': record['result_hash'], 'after_state_hash': result['after_state_hash']},
         'account_result': r}
    raw = canon(v)
    validate_public(raw, principal, context)
    return raw


def reconcile(saved, queried, principal, context):
    a, b = validate_public(saved, principal, context), validate_public(queried, principal, context)
    require(a == b and saved == queried, 'CLIENT_RECEIPT_MISMATCH')
    return a


def validate_request_receipt(raw, principal, context, kind, request_hash, saved_source=None):
    v = validate_public(raw, principal, context)
    r = v['account_result']
    require(r['kind'] == kind and r['request_hash'] == request_hash, 'CLIENT_RECEIPT_MISMATCH')
    if saved_source is not None:
        require(v['source'] == saved_source, 'CLIENT_RECEIPT_MISMATCH')
    return v


def verify_trusted(public, raw_frame, principal, context):
    validate_public(public, principal, context)
    require(public == project(raw_frame, principal, context), 'SOURCE_PROJECTION_MISMATCH')
