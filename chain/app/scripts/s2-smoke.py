#!/usr/bin/env python3
"""Bounded real CometBFT test; public synthetic keys. Not four-validator AT01."""
import argparse,base64,hashlib,json,pathlib,struct,subprocess,time,urllib.request,urllib.parse
p=argparse.ArgumentParser()
p.add_argument('--binary',required=True);p.add_argument('--output',required=True)
p.add_argument('--port',type=int,default=29757)
a=p.parse_args()
binary=str(pathlib.Path(a.binary).resolve())
out=pathlib.Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False)
home=out/'node';rpc=f'tcp://127.0.0.1:{a.port}'
ops=pathlib.Path(__file__).resolve().parents[1]/'config/operator-accounts.json'
def run(cmd,*args,ok=True):
 r=subprocess.run([binary,cmd,'--network','s2',*map(str,args)],capture_output=True,text=True,timeout=35)
 if ok and r.returncode:raise AssertionError(r.stderr+r.stdout)
 if not ok:assert r.returncode!=0
 return r
init=json.loads(run('init','--home',home,'--rpc',rpc,'--p2p',f'tcp://127.0.0.1:{a.port-1}','--operator-accounts',ops).stdout)
gh=init['genesis_hash']
(out/'init.json').write_text(json.dumps(init,indent=2))
proc=None;log=None
def start(name):
 global proc,log
 log=open(out/(name+'.log'),'w')
 proc=subprocess.Popen([binary,'start','--network','s2','--home',str(home),'--genesis-hash',gh],stdout=log,stderr=log)
 deadline=time.monotonic()+25
 while time.monotonic()<deadline:
  if proc.poll() is not None:raise AssertionError('node exited')
  r=subprocess.run([binary,'snapshot','--network','s2','--rpc',rpc],capture_output=True,text=True,timeout=5)
  if r.returncode==0:return
  time.sleep(.2)
 raise AssertionError('readiness timeout')
def stop():
 global proc
 if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=15)
 if log:log.close()
def snap(height=0):
 v=json.loads(run('snapshot','--rpc',rpc,'--height',height).stdout)
 b=v['body'];assert b['context']['genesis_hash']==gh
 assert b['context']['chain_id']=='nus-s2-dev-1'
 owners=[base64.b64decode(x['owner'],validate=True) for x in b['accounts']]
 assert owners==sorted(owners) and len(set(owners))==2
 for ac,owner in zip(b['accounts'],owners):
  pk=base64.b64decode(ac['public_key'],validate=True)
  assert len(pk)==1952 and hashlib.sha256(pk).digest()[:20]==owner
 for asset in b['supplies']:
  denom=asset['denom'];bal=[next(x for x in ac['balances'] if x['denom']==denom) for ac in b['accounts']]
  assert sum(int(x['confirmed_atoms']) for x in bal)==int(asset['module_atoms'])
  assert sum(int(x['bank_atoms']) for x in bal)+int(asset['module_atoms'])==int(asset['bank_supply_atoms'])==int(asset['genesis_supply_atoms'])==2000000000000
 raw=json.dumps(b,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode();domain=b'NUS/S2/SNAPSHOT/V1'
 assert hashlib.sha256(struct.pack('>I',len(domain))+domain+struct.pack('>Q',len(raw))+raw).hexdigest()==v['snapshot_id']
 h=b['observed_height']
 query=urllib.parse.urlencode({'path':'"/nus.exchange.v1.Query/Snapshot"','height':h,'data':'0x'})
 with urllib.request.urlopen(f'http://127.0.0.1:{a.port}/abci_query?{query}',timeout=5) as r:abci=json.load(r)
 assert abci['result']['response']['height']==h and abci['result']['response']['code']==0
 (out/f'abci-{h}.json').write_text(json.dumps(abci,indent=2))
 with urllib.request.urlopen(f'http://127.0.0.1:{a.port}/block?height={h}',timeout=5) as r:block=json.load(r)
 assert block['result']['block_id']['hash'].lower()==b['block_hash']
 assert block['result']['block']['header']['chain_id']==b['context']['chain_id']
 (out/f'block-{h}.json').write_text(json.dumps(block,indent=2))
 return v
def tx(label,user,denom,op,amount,rid,epoch=None,ok=True):
 args=['--rpc',rpc,'--user',user,'--denom',denom,'--op',op,'--amount',amount,'--request-id',f'{rid:064x}','--expiry','1000000']
 if epoch is not None:args+=['--epoch',epoch]
 r=run('tx',*args,ok=ok);(out/(label+'.json')).write_text(r.stdout or json.dumps({'error':r.stderr}))
 snap()
try:
 start('start')
 tx('base-deposit',0,'DEVBASE','deposit',10000000,1)
 tx('quote-deposit',1,'DEVQUOTE','deposit',100000000,1)
 tx('other-asset-deposit',0,'DEVQUOTE','deposit',1000000,2)
 before=snap();h=before['body']['observed_height']
 (out/'deposited-snapshot.json').write_text(json.dumps(before,indent=2))
 tx('base-withdraw',0,'DEVBASE','withdraw',1000000,3)
 tx('retry',0,'DEVBASE','withdraw',1000000,3,epoch='0')
 tx('stale-quote-epoch',0,'DEVQUOTE','withdraw',1,4,epoch='0',ok=False)
 tx('gas-rejected',0,'DEVGAS','deposit',1,5,ok=False)
 tx('quote-withdraw',1,'DEVQUOTE','withdraw',1000000,6)
 assert snap(h)==before
 signed=out/'signed.raw'
 run('tx','--rpc',rpc,'--denom','DEVBASE','--amount','1','--request-id',f'{7:064x}','--out',signed)
 bad=out/'bad.raw';data=bytearray(signed.read_bytes());data[-1]^=1;bad.write_bytes(data)
 r=run('broadcast','--rpc',rpc,'--file',bad,ok=False);(out/'bad-signature.txt').write_text(r.stdout+r.stderr)
 run('broadcast','--rpc',rpc,'--file',signed)
 r=run('broadcast','--rpc',rpc,'--file',signed,ok=False);(out/'raw-replay.txt').write_text(r.stdout+r.stderr)
 latest=snap();latest_h=latest['body']['observed_height']
 stop();start('restart')
 assert snap(latest_h)==latest
 (out/'final-snapshot.json').write_text(json.dumps(latest,indent=2))
 (out/'manifest.json').write_text(json.dumps({'status':'PASS','scope':'single-validator real CometBFT; four-validator integrated AT01 NOT_RUN',
 'genesis_sha256':gh,'binary_sha256':hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),
 'checks':['two assets actual signed deposit/withdraw','shared epoch stale quote rejection','same request retry once','GAS rejection','bad signature','raw replay','supply custody reconciliation','snapshot canonical hash/header/owner binding','historical H stable after changes and restart']},indent=2))
 print('PASS S2 real CometBFT smoke')
finally:stop()
