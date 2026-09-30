"""Check frozen synthetic vectors and S0 preservation; no product PASS claims."""
import json,hashlib,subprocess
from pathlib import Path
from codec import encode,b,address
root=Path(__file__).resolve().parents[1]
repo=root.parents[1]
def load(p): return json.loads(p.read_text())
v=load(root/'vectors/direct.json'); pk=bytes.fromhex(v['public_key_hex'])
for c in v['cases']:
    for k,x in encode(c,pk).items(): assert c[k]==x,(c['id'],k)
    assert c['owner']==address(hashlib.sha256(pk).digest()[:20])
    sig=bytes.fromhex(c['signature_hex']);assert len(sig)==3309
    raw=b(1,bytes.fromhex(c['body_hex']))+b(2,bytes.fromhex(c['auth_info_hex']))+b(3,sig)
    assert len(raw)<=16384
    assert c['tx_raw_hex']==raw.hex()
    assert c['tx_hash']==hashlib.sha256(raw).hexdigest().upper()
for c in load(root/'vectors/state.json')['cases']:
    before=list(map(int,c['before']));after=list(map(int,c['after']));a=int(c['amount'])
    for s in [before,after]:
        assert all(0<=x<2**128 for x in s[:3]);assert 0<=s[3]<2**64
        assert s[1]==s[2]
    assert sum(before[:2])==sum(after[:2])
    if c['expected']=='COMMITTED':
        B,C,U,E=before
        expected=[B-a,C+a,U+a,E] if c['op']=='deposit' else [B+a,C-a,U-a,E+1]
        assert after==expected
    else: assert after==before
    if c['expected']=='EXPIRED': assert int(c['height'])>=int(c['expiry_height'])
    if c['expected']=='EPOCH_MISMATCH':assert int(c['expected_epoch'])!=before[3]
    if c['expected']=='INSUFFICIENT_BANK_BALANCE':assert a>before[0]
    if c['expected']=='INSUFFICIENT_CONFIRMED_BALANCE':assert a>before[1]
    if c['expected']=='INTEGER_RANGE':assert before[3]==2**64-1
manifest=load(root/'manifest.json')
for path,digest in manifest['files'].items(): assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
baseline=manifest['s0_baseline']
diff=subprocess.check_output(['git','diff',baseline,'--','protocol/v1','chain/go.mod','chain/go.sum'],cwd=repo)
assert not diff,'S0 changed'
print('PASS: 3 SignDoc/TxRaw byte-hash fixtures; 10 synthetic conservation expectations; manifest; S0 unchanged')
print('NOT_RUN: SDK ante, Go/Rust/TS SDK differential, actual genesis/app/TX, S1-AT01..07')
