#!/usr/bin/env python3
"""Fail-closed oracle gate. Reports raw logs and exact per-lane case coverage."""
import argparse,datetime,hashlib,json,subprocess,sys,time
from pathlib import Path
def validate_receipt(receipt, expected, revision, vector_hash):
 if receipt.get('contract_revision')!=revision or receipt.get('vectors_sha256')!=vector_hash:raise ValueError('receipt revision/hash mismatch')
 rows=receipt.get('results')
 if not isinstance(rows,list):raise ValueError('missing results')
 ids=[r['id'] for r in rows]
 if len(ids)!=len(set(ids)):raise ValueError('duplicate result ID')
 if set(ids)!=set(expected):raise ValueError('missing/extra result IDs')
 for row in rows:
  if json.dumps(row['actual'],sort_keys=True)!=json.dumps(expected[row['id']],sort_keys=True):raise ValueError('oracle mismatch: '+row['id'])
def main():
 p=argparse.ArgumentParser();p.add_argument('--repo',type=Path,required=True);p.add_argument('--config',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
 root=a.repo.resolve();c=json.loads(a.config.read_text());report={'status':'FAIL','started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'lanes':{},'missing':[],'boundaries':c.get('boundaries')};a.output.parent.mkdir(parents=True,exist_ok=True)
 def run(name,argv,cwd,input=None):
  start=time.monotonic();r=subprocess.run(argv,cwd=cwd,input=input,capture_output=True,text=True,timeout=900)
  report.setdefault('commands',{})[name]={'argv':argv,'cwd':str(Path(cwd).relative_to(root)),'seconds':time.monotonic()-start,'exit_code':r.returncode,'stdout':r.stdout,'stderr':r.stderr}
  (a.output.parent/(name+'.stdout.log')).write_text(r.stdout);(a.output.parent/(name+'.stderr.log')).write_text(r.stderr)
  if r.returncode:raise ValueError(name+' failed')
  return r.stdout
 try:
  for k in ('contract_revision','vectors','vectors_sha256'):
   if not c.get(k):report['missing'].append(k)
  for lang in ('go','rust','ts'):
   if not c['lanes'][lang].get('vectors'):report['missing'].append(lang+':vector-command')
  if report['missing']:report['status']='BLOCKED';return 2
  if not c.get('cases') or not c.get('files_sha256'):raise ValueError('missing pinned oracle/source hashes')
  for f,h in c['files_sha256'].items():
   if hashlib.sha256((root/f).read_bytes()).hexdigest()!=h:raise ValueError('input hash mismatch: '+f)
  if hashlib.sha256((root/c['vectors']).read_bytes()).hexdigest()!=c['vectors_sha256']:raise ValueError('vector hash mismatch')
  ids=[x['id'] for x in c['cases']]
  if len(ids)!=len(set(ids)):raise ValueError('duplicate oracle ID')
  report['commit']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
  report['manifest_sha256']=hashlib.sha256(a.config.read_bytes()).hexdigest()
  run('manifest-check',['python3','ops/ci/build_manifest.py','--check'],root)
  run('protocol-reference',['python3','protocol/v1/tools/check.py'],root)
  for lang,lane in c['lanes'].items():
   cwd=root/lane['cwd']
   for phase in ('build','test'):run(lang+'-'+phase,lane[phase],cwd)
   selected=[x for x in c['cases'] if lang in x['lanes']]
   request=[dict(x['request'],id=x['id']) for x in selected]
   rows=json.loads(run(lang+'-vectors',lane['vectors'],cwd,json.dumps(request)))
   receipt={'contract_revision':c['contract_revision'],'vectors_sha256':c['vectors_sha256'],'results':rows}
   # Receipt metadata comes from verified immutable input, rows only from actual library calls.
   validate_receipt(receipt,{x['id']:x['expected'] for x in selected},c['contract_revision'],c['vectors_sha256'])
   report['lanes'][lang]={'status':'PASS','receipt':receipt,'scope_counts':{s:sum(x['scope']==s for x in selected) for s in {x['scope'] for x in selected}}}
  run('rc4-full-output',['python3','ops/ci/rc4.py',str(a.output.parent.resolve()/'rc4')],root)
  report['rc4']=json.loads((a.output.parent/'rc4/rc4.json').read_text())
  run('settlement-regression',['python3','settlement/v1/test_contract.py'],root)
  before={f:hashlib.sha256((root/f).read_bytes()).hexdigest() for f in ['settlement/v1/api.schema.json','settlement/v1/fixtures.json']}
  run('settlement-generate',['python3','settlement/v1/generate.py'],root)
  if before!={f:hashlib.sha256((root/f).read_bytes()).hexdigest() for f in before}:raise ValueError('E generated artifacts changed')
  report['e_consumed_hashes']={f:c['files_sha256'][f] for f in ['protocol/v1/schema.json','protocol/v1/vectors/message-codec.json','protocol/v1/vectors/s0-cases.json']}
  report['status']='PASS';return 0
 except Exception as e:report['error']=str(e);return 1
 finally:
  report['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat();a.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['status','missing','error'] if k in report}))
if __name__=='__main__':sys.exit(main())
