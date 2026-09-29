"""Audit S0 reproduction evidence; success means evidence consistency, not gate PASS."""
import hashlib,json,pathlib,sys
root=pathlib.Path(__file__).resolve().parents[1]
e=root/'security/evidence'; out=root/'qa/evidence';out.mkdir(exist_ok=True)
rows=json.loads((e/'differential.json').read_text())
expected={('fee-cap-u32','Rust'),('fee-ge-receive','Go'),('fee-ge-receive','TS'),('registered-key-type','Rust'),('fee-0-0','Go'),('fee-0-0','Rust'),('fee-1000-10001','TS')}
assert len(rows)==420 and len({(r['id'],r['language']) for r in rows})==420
assert {(r['id'],r['language']) for r in rows if not r['passed']}==expected
matrix={}
for prefix,count in [('matrix-',108),('matrix-codec-',27),('matrix-domain-',27),('matrix-context-',27)]:
 selected=[r for r in rows if r['id'].startswith(prefix)]
 assert len(selected)==count and all(r['passed'] for r in selected)
 matrix[prefix]={'count':count,'passed':True}
manifest=json.loads((root/'protocol/v1/manifest.candidate.json').read_text())
for name,digest in manifest['files_sha256'].items():
 assert hashlib.sha256((root/'protocol/v1'/name).read_bytes()).hexdigest()==digest,name
settlement=json.loads((e/'settlement-independent.json').read_text())
assert len(settlement)==20 and all(r['passed'] for r in settlement)
summary=json.loads((e/'summary.json').read_text());assert summary['total']==420 and summary['passed']==413
ci=out/'g-ci/s0-g-security-evidence/summary.json'
ci_equal=json.loads(ci.read_text())==summary if ci.exists() else None
if ci.exists():assert ci_equal
result={'evidence_consistency':'PASS','s0_gate':'FAIL','product_T01_T16':'NOT_RUN','unique_comparisons':420,'matched':413,'differences':7,'matrix_subsets':matrix,'manifest_files_verified':len(manifest['files_sha256']),'ci_summary_equal':ci_equal,'summary_sha256':hashlib.sha256((e/'summary.json').read_bytes()).hexdigest()}
(out/'audit.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
