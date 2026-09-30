"""Explicit maintainer regeneration; check.py never rewrites goldens."""
import json,hashlib
from pathlib import Path
from codec import encode,address
root=Path(__file__).resolve().parents[1]
s0=json.loads((root.parent/'v1/vectors/signatures.json').read_text())
pk=bytes.fromhex(s0['positives'][0]['public_key_hex'])
cases=[]
for i,(op,seq,acc,epoch,amount) in enumerate([('Deposit','0','0','0','1000000'),('Withdraw','1','0','0','400000'),('Deposit','2','7','1','1')]):
    c=dict(id='S1-DIRECT-%02d'%(i+1),type_url='/nus.exchange.v1.Msg'+op,owner=address(hashlib.sha256(pk).digest()[:20]),amount_atoms=amount,request_id=('%02x'%(i+1))*32,expected_epoch=epoch,expiry_height='100',genesis_hash='11'*32,chain_id='nus-s1-dev-1',account_number=acc,sequence=seq,fee_atoms='1000',gas_limit='500000')
    c.update(encode(c,pk)); cases.append(c)
out=dict(profile='SYNTHETIC_NOT_RUNTIME',test_seed_hex=s0['test_seed_hex'],public_key_hex=pk.hex(),cases=cases)
(root/'vectors/direct.json').write_text(json.dumps(out,indent=2)+'\n')
