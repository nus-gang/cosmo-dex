#!/usr/bin/env python3
"""CI wiring. Missing integration inputs BLOCK (exit 2), never silently pass."""
import argparse, hashlib, json, subprocess, sys
from pathlib import Path
p=argparse.ArgumentParser()
p.add_argument('--repo',type=Path,required=True)
p.add_argument('--config',type=Path,default=Path(__file__).resolve().parent/'manifest.json')
p.add_argument('--output',type=Path,required=True)
a=p.parse_args(); c=json.loads(a.config.read_text()); report={'status':'BLOCKED','lanes':{},'missing':[]}
for k in ('contract_revision','vectors','vectors_sha256'):
 if not c.get(k): report['missing'].append(k)
for lang in ('go','rust','ts'):
 lane=c['lanes'][lang]
 if not (a.repo/lane['cwd']).is_dir(): report['missing'].append(lang+':cwd')
 if not lane.get('vectors'): report['missing'].append(lang+':vector-command')
try:
 if report['missing']: sys.exit(2)
 vector=a.repo/c['vectors']
 if hashlib.sha256(vector.read_bytes()).hexdigest()!=c['vectors_sha256']: raise ValueError('vector hash mismatch')
 outputs=[]
 for lang in ('go','rust','ts'):
  lane=c['lanes'][lang]; result={}; report['lanes'][lang]=result
  for phase in ('build','test','vectors'):
   argv=lane[phase]
   if not isinstance(argv,list) or not argv or not all(isinstance(x,str) for x in argv): raise ValueError('argv array required')
   # Literal arguments, no shell expansion; {vectors} is the sole placeholder.
   argv=[x.replace('{vectors}',str(vector.resolve())) for x in argv]
   run=subprocess.run(argv,cwd=a.repo/lane['cwd'],capture_output=True,text=True,timeout=600)
   result[phase]={'exit_code':run.returncode,'stdout':run.stdout,'stderr':run.stderr}
   if run.returncode: raise ValueError(lang+':'+phase+' failed')
   if phase=='vectors': outputs.append(json.loads(run.stdout))
 # Agreed runners emit {contract_revision, vectors_sha256, results:[{id,sign_bytes_hex,valid,...}]}.
 for o in outputs:
  if o.get('contract_revision')!=c['contract_revision'] or o.get('vectors_sha256')!=c['vectors_sha256'] or not o.get('results'): raise ValueError('invalid vector receipt')
 if outputs[0]!=outputs[1] or outputs[1]!=outputs[2]: raise ValueError('cross-language mismatch')
 report['status']='PASS'
except Exception as e:
 report['status']='FAIL'; report['error']=str(e); sys.exit(1)
finally:
 a.output.parent.mkdir(parents=True,exist_ok=True)
 a.output.write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps({'status':report['status'],'missing':report['missing']}))
