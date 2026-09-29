"""Independent reference API mapping and fixture decisions, not product validation."""
from pathlib import Path
import base64, hashlib, json, re, runpy
r = Path(__file__).resolve().parents[1]
schema = json.loads((r / 'schema.json').read_text())
legacy = runpy.run_path(str(r / 'tools/verify-bytes.py'))
wire = runpy.run_path(str(r / 'tools/check-wire.py'))

def atoms(value):
    assert isinstance(value, str) and re.fullmatch(r'0|[1-9][0-9]*', value)
    n = int(value)
    assert n < 2**128
    return n.to_bytes(16, 'big')

amounts = json.loads((r / 'vectors/amount-codec.json').read_text())['cases']
for c in amounts:
    try:
        if 'api_json' in c:
            b = atoms(c['api_json'])
            assert b.hex() == c['wire_hex']
            assert base64.b64encode(b).decode() == c['wire_base64']
            assert str(int.from_bytes(b, 'big')) == c['api_json']
        else:
            assert len(bytes.fromhex(c['wire_hex'])) == 16
        result = 'OK'
    except AssertionError:
        result = 'INTEGER_RANGE' if 'api_json' in c else 'NON_CANONICAL_WIRE'
    assert result == c['expected'], c

# Encode API JSON independently of fixture fields/legacy encode.
def varint(n):
    out = bytearray()
    while True:
        out.append(n % 128 | (128 if n >= 128 else 0))
        n //= 128
        if not n:
            return bytes(out)

def encode_api(name, obj):
    assert set(obj) == {f['name'] for f in schema[name]}
    out = b''
    for f in schema[name]:
        t, v = f['type'], obj[f['name']]
        if t in ('u32', 'u64'):
            assert isinstance(v, str) and re.fullmatch(r'0|[1-9][0-9]*', v)
            assert int(v) < 2**int(t[1:])
            out += varint(f['tag'] << 3) + varint(int(v))
            continue
        if t == 'atoms':
            b = atoms(v)
        elif t == 'a':
            b = base64.b64decode(v, validate=True)
            assert len(b) == 20 and base64.b64encode(b).decode() == v
        elif t == 'h':
            assert re.fullmatch('[0-9a-f]{64}', v)
            b = bytes.fromhex(v)
        else:
            b = v.encode('ascii')
        out += varint(f['tag'] << 3 | 2) + varint(len(b)) + b
    return out

vectors = json.loads((r / 'vectors/message-codec.json').read_text())
by_id = {v['id']: v for v in vectors['positives']}
batches = json.loads((r / 'vectors/batches.json').read_text())['batches']
for v in by_id.values():
    raw = encode_api(v['message'], v['api_json'])
    assert raw.hex() == v['canonical_hex'], v['id']
    assert legacy['encode'](v['fields']) == raw
    wire['check'](raw, v['message'])
    if v['message'] == 'TransferStableV1':
        domain = b'NUS/PAYMENT_ID/V1'
        framed = len(domain).to_bytes(4, 'big') + domain + len(raw).to_bytes(8, 'big') + raw
        assert framed.hex() == v['payment_frame_hex']
        assert hashlib.sha256(framed).hexdigest() == v['payment_hash']
        if v['id'].startswith('transfer-change-'):
            assert v['payment_hash'] != by_id['transfer']['payment_hash']
    if 'source_batch_id' in v:
        batch = next(b for b in batches if b['id'] == v['source_batch_id'])
        bf = {tag: value for tag, kind, value in batch['fields']}
        a = v['api_json']
        assert (a['chain_id'], a['genesis_hash'], a['market_id'], a['batch_seq'], a['batch_id'], a['batch_hash']) == (bf[2], bf[10], bf[3], bf[5], batch['batch_id'], batch['batch_hash'])

for c in vectors['wire_cases']:
    try:
        wire['check'](bytes.fromhex(c['wire_hex']), c['message'])
        result = 'CANONICAL'
    except (AssertionError, UnicodeError):
        result = 'NON_CANONICAL_WIRE'
    assert result == c['expected'], c['id']

cfg = json.loads((r / 'dev-config.json').read_text())['transfer']
def decide(c):
    submitted = by_id[c['submitted']]
    a = submitted['api_json']
    stored = by_id[c['stored']] if c['stored'] else None
    if c['kind'] == 'receipt':
        keys = ('chain_id', 'genesis_hash', 'market_id', 'batch_seq')
        if any(a[k] != stored['api_json'][k] for k in keys):
            return 'NOT_FOUND_AT_HEIGHT'
        assert int(a['batch_seq']) < int(c['last_seq'])
        return 'ALREADY_COMMITTED' if all(a[k] == stored['api_json'][k] for k in ('batch_id', 'batch_hash')) else 'BATCH_CONFLICT'
    keys = ('sender', 'payment_id')
    if stored and all(a[k] == stored['api_json'][k] for k in keys):
        return 'ALREADY_COMMITTED' if submitted['payment_hash'] == stored['payment_hash'] else 'PAYMENT_ID_CONFLICT'
    if int(c['height']) >= int(a['expiry_height']):
        return 'EXPIRED'
    if not int(cfg['min_atoms']) <= int(a['amount_atoms']) <= int(cfg['max_atoms']):
        return 'MARKET_LIMIT'
    return 'OK'
for c in vectors['state_cases']:
    assert decide(c) == c['expected'], c['id']
print(f"PASS amount codec {len(amounts)}, complete message bytes {len(by_id)}, wire {len(vectors['wire_cases'])}, state examples {len(vectors['state_cases'])}; reference only")
