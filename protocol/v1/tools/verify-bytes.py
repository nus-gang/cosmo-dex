"""Independent Python encoding/hash comparison; not a hostile-input decoder."""
import json,hashlib,struct
from pathlib import Path
r=Path(__file__).resolve().parents[1] / 'vectors'
def vi(n):
    out=[]
    while n>=128: out.append((n&127)|128);n>>=7
    return bytes(out+[n])
def encode(fields):
    out=b''
    for tag,kind,value in fields:
        if kind in ('u32','u64'):
            n=int(value);assert 0<=n<2**int(kind[1:])
            out+=vi(tag*8)+vi(n)
        else:
            b=bytes.fromhex(value) if kind=='hex' else value.encode('ascii')
            out+=vi(tag*8+2)+vi(len(b))+b
    return out
def frame(domain,b):
    d=domain.encode('ascii')
    return struct.pack('>I',len(d))+d+struct.pack('>Q',len(b))+b
def h(b):return hashlib.sha256(b).hexdigest()
s=json.loads((r/'signatures.json').read_text())
for p in s['positives']:
    b=encode(p['fields']);assert b.hex()==p['canonical_hex']
    f=frame(bytes.fromhex(p['domain_hex']).decode(),b)
    assert f.hex()==p['sign_input_hex'] and h(f)==p['sha256']
    fields={t:(k,v) for t,k,v in p['fields']}
    if p['id']=='order':
        assert len(bytes.fromhex(fields[9][1]))==32
        assert fields[15]==('utf8','RECEIVE_ASSET_V1')
    if p['id']=='cancel':
        assert all(len(bytes.fromhex(fields[t][1]))==32 for t in [7,8,9])
        assert fields[11]==('utf8','x/exchange')
    if p['id']=='wallet':
        assert fields[4][1] in ['exchange-api','private-ws']
        assert 0<int(fields[7][1])-int(fields[9][1])<=120
        assert len(bytes.fromhex(fields[8][1]))==32
b=json.loads((r/'batches.json').read_text())
for f in b['fills']:
    t=encode(f['tuple_fields']);assert t.hex()==f['tuple_hex']
    framed=frame('NUS/FILL_ID/V1',t)
    assert framed.hex()==f['fill_id_input_hex'] and h(framed)==f['fill_id']
    assert encode(f['fields']).hex()==f['canonical_hex']
for v in b['batches']:
    assert all(t!=7 for t,k,x in v['core_fields'])
    core=encode(v['core_fields']);assert core.hex()==v['core_hex']
    framed=frame('NUS/BATCH_ID/V1',core)
    assert framed.hex()==v['batch_id_input_hex'] and h(framed)==v['batch_id']
    assert [f for f in v['fields'] if f[0]!=7]==v['core_fields']
    raw=encode(v['fields']);assert raw.hex()==v['canonical_or_candidate_hex']
    framed=frame('NUS/BATCH_HASH/V1',raw)
    assert framed.hex()==v['batch_hash_input_hex'] and h(framed)==v['batch_hash']
print('PASS Python independent bytes/hash comparison: 3 signatures, 2 fills, 10 batches; no crypto/parser/state verification')
