#!/usr/bin/env python3
"""Run a real single-validator CometBFT smoke. No mocks; public test keys only."""
import argparse, hashlib, json, pathlib, subprocess, time
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--output',required=True);p.add_argument('--port',type=int,default=28757);a=p.parse_args()
binary=str(pathlib.Path(a.binary).resolve());out=pathlib.Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False);home=out/'node';rpc=f'tcp://127.0.0.1:{a.port}';p2p=f'tcp://127.0.0.1:{a.port-1}'
def run(*args,success=True):
 r=subprocess.run([binary,*args],capture_output=True,text=True,timeout=40)
 if success and r.returncode: raise RuntimeError(f'{args}: {r.stderr} {r.stdout}')
 return r
init=json.loads(run('init','--home',str(home),'--rpc',rpc,'--p2p',p2p).stdout);gh=init['genesis_hash'];(out/'init.json').write_text(json.dumps(init,indent=2))
proc=None;log=None

def start(label):
 global proc,log
 log=open(out/(label+'.log'),'w');proc=subprocess.Popen([binary,'start','--home',str(home),'--genesis-hash',gh],stdout=log,stderr=log)
 deadline=time.monotonic()+20
 while time.monotonic()<deadline:
  if proc.poll() is not None: raise RuntimeError('node exited: '+(out/(label+'.log')).read_text()[-4000:])
  r=run('snapshot','--rpc',rpc,success=False)
  if r.returncode==0:return json.loads(r.stdout)
  time.sleep(.2)
 raise RuntimeError('node readiness timeout')
def stop():
 global proc,log
 if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=20)
 if log:log.close()

def snapshot():return json.loads(run('snapshot','--rpc',rpc).stdout)
def conserved(s):
 assert sum(int(u['exchange_atoms']) for u in s['accounts'])==int(s['module_atoms'])
 assert sum(int(u['bank_atoms']) for u in s['accounts'])+int(s['module_atoms'])==2000000000000
 assert sum(int(u['gas_atoms']) for u in s['accounts'])+int(s['gas_collector_atoms'])==2000000000

def tx(label,user,op,amount,rid,extra=(),success=True):
 r=run('tx','--rpc',rpc,'--user',str(user),'--op',op,'--amount',str(amount),'--request-id',f'{rid:064x}','--expiry','1000000',*extra,success=success)
 if success:
  d=json.loads(r.stdout);assert d['height']>0 and d['check_tx']['code']==0 and d['tx_result']['code']==0
 else:
  assert r.returncode!=0;d=json.loads(r.stdout) if r.stdout else {'stderr':r.stderr}
 (out/(label+'.json')).write_text(json.dumps(d,indent=2));conserved(snapshot());return d
try:
 before=start('first-start');conserved(before)
 for u in range(2):
  tx(f'user{u}-deposit',u,'deposit',1000000,1)
  tx(f'user{u}-withdraw',u,'withdraw',400000,2)
  receipt=json.loads(run('receipt','--rpc',rpc,'--user',str(u),'--request-id',f'{2:064x}').stdout)
  tx(f'user{u}-retry',u,'withdraw',400000,2,('--epoch','0'))
  assert receipt==json.loads(run('receipt','--rpc',rpc,'--user',str(u),'--request-id',f'{2:064x}').stdout)
  tx(f'user{u}-overdraft',u,'withdraw',600001,3,success=False)
  tx(f'user{u}-id-conflict',u,'deposit',1,2,success=False)
 raw=out/'signed.raw'
 run('tx','--rpc',rpc,'--user','0','--amount','1','--request-id',f'{4:064x}','--out',str(raw))
 bad=out/'bad.raw';data=bytearray(raw.read_bytes());data[-1]^=1;bad.write_bytes(data)
 r=run('broadcast','--rpc',rpc,'--file',str(bad),success=False);assert r.returncode!=0
 (out/'bad-signature.txt').write_text(r.stdout+r.stderr)
 r=run('broadcast','--rpc',rpc,'--file',str(raw));d=json.loads(r.stdout);assert d['tx_result']['code']==0
 r=run('broadcast','--rpc',rpc,'--file',str(raw),success=False);assert r.returncode!=0
 (out/'duplicate-raw.txt').write_text(r.stdout+r.stderr)
 pre=snapshot();conserved(pre);receipts=[json.loads(run('receipt','--rpc',rpc,'--user',str(u),'--request-id',f'{2:064x}').stdout) for u in range(2)]
 stop();post=start('restart');conserved(post)
 for s in (pre,post):s.pop('observed_height')
 assert pre==post
 assert receipts==[json.loads(run('receipt','--rpc',rpc,'--user',str(u),'--request-id',f'{2:064x}').stdout) for u in range(2)]
 (out/'ledger.json').write_text(json.dumps(post,indent=2));(out/'receipts.json').write_text(json.dumps(receipts,indent=2))
 manifest={'scope':'real single-validator CometBFT; not AT01/AT05','version':json.loads(run('version').stdout),'binary_sha256':hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),'genesis_sha256':gh,'config_sha256':hashlib.sha256((home/'config/config.toml').read_bytes()).hexdigest(),'checks':['two-user deposit/withdraw','same request re-sign no double pay','overdraft rollback','ID conflict','bad signature','same TxRaw replay rejection','DEVQUOTE and DEVGAS conservation','restart ledger and receipts identical'],'status':'PASS'}
 (out/'manifest.json').write_text(json.dumps(manifest,indent=2));print(json.dumps(manifest,indent=2))
finally:stop()
