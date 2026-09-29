"""Wallet scoped replay; does not replace Security's complete independent matrix.
Usage: python3 web/test/cross-enum/compare.py SECURITY_CHECKOUT OUTPUT_DIR
Security binaries must be prepared at 068477c via its README first.
"""
import copy, hashlib, json, pathlib, subprocess, sys
here=pathlib.Path(__file__).resolve().parent
sec=pathlib.Path(sys.argv[1]).resolve(); out=pathlib.Path(sys.argv[2]).resolve();out.mkdir(parents=True,exist_ok=True)
commands={'Go':[str(sec/'security/go-runner')], 'Rust':[str(sec/'exchange/target/debug/examples/security')], 'TS':['node','--experimental-strip-types',str(here/'runner.ts')]}
ps={k:subprocess.Popen(v,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,cwd=sec) for k,v in commands.items()}
def call(lang,r):
 p=ps[lang];p.stdin.write(json.dumps(r)+'\n');p.stdin.flush();return json.loads(p.stdout.readline())
inputs=json.loads((here/'inputs.json').read_text())
base=next(c for c in inputs if c['id']=='rc3-signed-registered-normal')
seed=json.loads((sec/'protocol/v1/vectors/signatures.json').read_text())['test_seed_hex']
for side in ['1','2','3']:
 for kind in ['1','2','3']:
  case=copy.deepcopy(base);r=case['request'];r['API'].update(side=side,order_type=kind)
  enc=call('TS',dict(Op='encode',Name='OrderV1',API=r['API'],Domain='NUS/ORDER/V1'))
  signed=call('TS',dict(Op='sign',Seed=seed,Msg=enc['msg'],Context=''))
  r.update(Wire=enc['wire'],Sig=signed['sig'])
  case['id']=f'enum-{side}-{kind}'
  if side=='3' or kind=='3':case['expected']['decision']['snapshot_policy'].update(status='REJECTED',code='MARKET_LIMIT')
  inputs.append(case)
rows=[]
try:
 for case in inputs:
  for lang in ps:
   actual=call(lang,case['request']);expected=case['expected']
   if isinstance(expected,dict):ok=actual==expected
   else:
    d=actual.get('decision',{});ok=d.get('authentication',{}).get('status')=='PASS' and d.get('snapshot_policy',{}).get('status') in ['REJECTED','NOT_CONNECTED'] and d.get('ack')=='NOT_CONNECTED'
   rows.append(dict(id=case['id'],language=lang,expected=expected,actual=actual,passed=ok))
finally:
 for p in ps.values():p.stdin.close();assert p.wait()==0
(out/'inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
(out/'results.json').write_text(json.dumps(rows,indent=2)+'\n')
summary={lang:dict(total=sum(r['language']==lang for r in rows),passed=sum(r['language']==lang and r['passed'] for r in rows),failures=[r['id'] for r in rows if r['language']==lang and not r['passed']]) for lang in ps}
summary['input_sha256']=hashlib.sha256((out/'inputs.json').read_bytes()).hexdigest()
summary['binary_sha256']={k:hashlib.sha256(pathlib.Path(commands[k][0]).read_bytes()).hexdigest() for k in ['Go','Rust']}
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2))
sys.exit(any(not r['passed'] for r in rows))
