"""QA independent artifact/source audit; run after the documented fresh executions."""
import collections, hashlib, json, pathlib, subprocess
R=pathlib.Path(__file__).resolve().parents[1]; Q=R/'qa-rc4'; B=R.parent/'NUS-17-rc4-ci'
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=R)
rows=read(R/'security/evidence/differential.json')
assert len(rows)==969 and all(x['passed'] and all(k in x['actual'] and x['actual'][k]==v for k,v in x['expected'].items()) for x in rows)
ids=[(x['id'],x['language']) for x in rows];assert len(set(ids))==969
old=read(Q/'evidence/prior-783.json')
oldids={(x['id'],x['language']) for x in old};assert len(oldids)==783 and oldids<=set(ids)
gci=Q/'evidence/ci/g/s0-g-security-evidence'
assert read(gci/'differential.json')==rows
for k,sha1 in [('g','041fa5954f3fa950b100c7a505217d949165347b'),('b','178aaaf1253ef8573ec7158e02291d92b40e3ea3')]:
 run=read(Q/f'evidence/ci/{k}-run.json');assert run['headSha']==sha1 and run['conclusion']=='success'
local=read(B/'.evidence/vectors.json'); remote=read(next((Q/'evidence/ci/b').glob('s0-vectors-*/vectors.json')))
assert local['status']==remote['status']=='PASS' and local['missing']==[]
for key in ['lanes','rc4','manifest_sha256','commit','boundaries','e_consumed_hashes']:assert local[key]==remote[key],key
assert local['manifest_sha256']==sha(B/'ops/ci/manifest.json')=='70c09b2759d1d649ef829b8f90d46418846eba83451c324c6525330b9ead319f'
assert sha(B/'ops/ci/cto-input.json')=='05abeba166e00de2de12be35bd36c186a706cc9aada93ff666840927f2da7e9d'
source=read(B/'ops/ci/cto-input.json')['inputs']; counts={}
for label,row in source.items():
 paths=git('ls-tree','-r','--name-only',row['commit'],row['path']).decode().splitlines()
 assert git('rev-parse',row['commit']+':'+row['path']).decode().strip()==row['tree']
 for p in paths:
  original=git('show',row['commit']+':'+p)
  assert original==(B/p).read_bytes(),('B original',p)
  assert original==(R/p).read_bytes(),('G original',p)
 counts[label]=len(paths)
for row in read(R/'security/evidence/inputs.json'):
 assert any(row['sha']==v['commit'] and row['component']==v['path'] for v in source.values())
m=read(R/'protocol/v1/manifest.candidate.json')
for p,h in m['files_sha256'].items():assert sha(R/'protocol/v1'/p)==h
snapshot=read(R/'protocol/v1/vectors/snapshot-output.json')['cases'];expected={v['id']:v['expected'] for v in snapshot}
for lane,data in local['rc4']['lanes'].items():
 assert len(data['results'])==60 and len({v['id'] for v in data['results']})==60
 for v in data['results']:assert v['actual']==expected[v['id']]
browser=read(Q/'evidence/browser/browser.json'); cib=read(gci/'browser/browser.json')
assert browser['suite']==cib['suite'] and browser['suite']['passed']==453
assert not browser['overflow'] and browser['pageErrors']==[] and browser['externalRequests']==0
harness=git('diff','041fa5954f3fa950b100c7a505217d949165347b','a2fb8e83870e60ff11134a7d6c71c2605fed3ef5','--','security/*.py','security/*.go','security/*.rs','security/*.ts','security/*.sh','.github')
assert not harness
result=dict(status='PASS',comparisons=969,passed=969,missing=0,duplicates=0,prior_ids_retained=783,added=186,
 languages=dict(collections.Counter(x['language'] for x in rows)),
 crypto_groups={p:sum(x['id'].startswith(p) for x in rows) for p in ['matrix-','matrix-context-','matrix-domain-','matrix-codec-']},
 source_files_checked=counts,protocol_files_checked=len(m['files_sha256']),
 exact_full_rows=sum(x['actual']==x['expected'] for x in rows),expected_projection_rows=sum(x['actual']!=x['expected'] for x in rows),
 B_lane_counts={k:len(v['receipt']['results']) for k,v in local['lanes'].items()},
 B_rc4_counts={k:v['count'] for k,v in local['rc4']['lanes'].items()},
 G_local_ci_full_rows_equal=True,B_local_ci_results_equal=True,browser_checks=453,
 B_sha=local['commit'],G_sha='a2fb8e83870e60ff11134a7d6c71c2605fed3ef5',
 contract_sha256=m['contract_sha256'],vectors_sha256=m['vectors_sha256'],
 candidate_status_preserved=m['status'],S0_gate='PASS',product_T01_T16='NOT_RUN')
(Q/'evidence/audit.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
