"""Maintainer-only: seal generated signatures into TxRaw and freeze manifest."""
import hashlib,json
from pathlib import Path
from codec import b
root=Path(__file__).resolve().parents[1]
p=root/'vectors/direct.json';v=json.loads(p.read_text())
for c in v['cases']:
    raw=b(1,bytes.fromhex(c['body_hex']))+b(2,bytes.fromhex(c['auth_info_hex']))+b(3,bytes.fromhex(c['signature_hex']))
    c['tx_raw_hex']=raw.hex();c['tx_hash']=hashlib.sha256(raw).hexdigest().upper()
p.write_text(json.dumps(v,indent=2)+'\n')
manifest=dict(contract_version='s1-dev-1-rc1',s0_baseline='df4da26463824de4f9450a478047d3c07d8fc308',sdk='v0.55.0',cometbft='v0.40.0',app_go='1.26.5',app_execution='NOT_RUN',files={})
for p in sorted(root.rglob('*')):
    if p.is_file() and '__pycache__' not in p.parts and p.name not in ['manifest.json','verification.txt']:
        manifest['files'][str(p.relative_to(root))]=hashlib.sha256(p.read_bytes()).hexdigest()
(root/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
