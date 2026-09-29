"""Run 60 rc4 original IDs through actual signed-order entrypoints in all lanes.
Rust's reviewed test creates mock-key signed requests; expectations always come
from A's source oracle. No historical evidence file is read.
"""
import json, os, subprocess, sys
from pathlib import Path
from vectors import validate_receipt
root=Path(__file__).resolve().parents[2]
out=Path(sys.argv[1]).resolve();out.mkdir(parents=True,exist_ok=True)
oracle=json.loads((root/'protocol/v1/vectors/snapshot-output.json').read_text())['cases']
expected={c['id']:c['expected'] for c in oracle}
assert len(expected)==len(oracle)==60
report={'status':'FAIL','historical_G':'783/782/1 FAIL preserved','scope':'actual ML-DSA authentication; synthetic snapshot; no ACK/ledger','lanes':{}}
def run(name,cmd,cwd,input=None,env=None):
 r=subprocess.run(cmd,cwd=cwd,input=input,text=True,capture_output=True,env=env,timeout=900)
 (out/(name+'.stdout.log')).write_text(r.stdout);(out/(name+'.stderr.log')).write_text(r.stderr)
 if r.returncode:raise RuntimeError(name+' failed: '+r.stderr[-2000:])
 return r.stdout
try:
 target=out/'rust-generated';target.mkdir(exist_ok=True)
 evidence=target/'rc4-full-outputs.json'
 if evidence.exists():evidence.unlink()
 run('rust-rc4',['cargo','test','--locked','--test','rc4','--','--nocapture'],root/'exchange',env={**os.environ,'NUS_RC4_EVIDENCE':str(target)})
 rows=json.loads(evidence.read_text());requests=[{'id':r['id'],'op':'rc4','native':r['request']} for r in rows]
 for lane,cmd,cwd in [('rust',None,None),('go',['go','run','-mod=readonly','../ops/ci/ports/go.go'],root/'chain'),('ts',['node','--experimental-strip-types','../ops/ci/ports/ts.ts'],root/'web')]:
  results=[{'id':r['id'],'actual':r['actual']} for r in rows] if lane=='rust' else json.loads(run(lane+'-rc4',cmd,cwd,json.dumps(requests)))
  validate_receipt({'contract_revision':'rc4','vectors_sha256':'verified-by-parent-gate','results':results},expected,'rc4','verified-by-parent-gate')
  report['lanes'][lane]={'status':'PASS','count':len(results),'results':results}
 report['status']='PASS'
finally:
 (out/'rc4.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'status':report['status'],'counts':{k:v['count'] for k,v in report['lanes'].items()}}))
