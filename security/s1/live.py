"""Independent, bounded four-validator security test; public synthetic fixtures only."""
import argparse,base64,hashlib,json,subprocess,sys,urllib.request
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'ops/s1'))
from devnet import init,rpc,cli
from integration import eventually
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--home',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=False)
a.operators=ROOT/'chain/app/config/operator-accounts.json';a.base_port=32656
EXPECTED={'wrong_chain':'signature verification failed','wrong_account':'signature verification failed','owner_key_mismatch':'UNAUTHORIZED','unregistered_key':'UNAUTHORIZED','signature_tamper':'signature verification failed','fee_granter':'UNSUPPORTED_OPTION','fee_payer':'UNSUPPORTED_OPTION','wrong_fee_denom':'INVALID_FEE','insufficient_fee':'INSUFFICIENT_FEE','gas_overflow':'INVALID_FEE','bank_bypass':'unknown request','wrong_genesis':'WRONG_CONTEXT','zero':'INTEGER_RANGE','negative':'NON_CANONICAL_INPUT','leading_zero':'NON_CANONICAL_INPUT','fraction':'NON_CANONICAL_INPUT','max_plus_one':'INTEGER_RANGE','u128_overflow':'INTEGER_RANGE','epoch_overflow':'INTEGER_RANGE','expired':'EXPIRED','overdraft':'INSUFFICIENT_CONFIRMED_BALANCE','race_second':'incorrect account sequence','empty_bank':'INSUFFICIENT_BANK_BALANCE'}
report={'status':'FAIL','candidate':'5971e98481c832a811eeb3b01efc622a0dcded25','cases':[]};proc=None;log=None
def verify_success(report):
 amounts={'positive_control':(0,1,False),'positive_control_return':(0,1,True),'deposit0':(0,1000000,False),'deposit1':(1,1000000,False),'withdraw0':(0,400000,True),'withdraw1':(1,400000,True),'race_first':(0,400000,True),'direct_recovery':(0,200000,True),'maximum':(0,1000000000000,False),'maximum_return':(0,1000000000000,True)}
 for c in report['cases']:
  if c['name'] not in amounts:continue
  user,amount,withdraw=amounts[c['name']]
  before=next(x for x in c['before']['accounts'] if x['account_number']==str(user))
  after=next(x for x in c['after']['accounts'] if x['account_number']==str(user))
  delta=-amount if withdraw else amount
  assert int(after['exchange_atoms'])==int(before['exchange_atoms'])+delta,c['name']
  assert int(after['bank_atoms'])==int(before['bank_atoms'])-delta,c['name']
  assert int(after['epoch'])==int(before['epoch'])+int(withdraw),c['name']
  assert c['tx_hash']==c['response']['result']['hash'],c['name']
 assert {v['voting_power'] for v in report['consensus']['validators']['validators']}=={'10'}
try:
 m=init(a);report['manifest']=m
 for node in m['nodes']:
  (a.output/f"node{node['index']}-config.toml").write_bytes((Path(node['home'])/'config/config.toml').read_bytes())
 (a.output/'genesis.json').write_bytes((Path(m['nodes'][0]['home'])/'config/genesis.json').read_bytes())
 log=open(a.output/'supervisor.log','w');proc=subprocess.Popen([sys.executable,str(ROOT/'ops/s1/devnet.py'),'serve','--home',str(a.home)],stdout=log,stderr=log)
 node=m['nodes'][0]
 eventually(lambda: min(int(rpc(n,'/status')['sync_info']['latest_block_height']) for n in m['nodes'])>=3)
 def snapshot():return cli(m['binary'],'snapshot','--rpc',node['rpc'])
 def stable(s):return {k:v for k,v in s.items() if k not in ('observed_height',)}
 def conserve(s):
  assert sum(int(x['bank_atoms']) for x in s['accounts'])+int(s['module_atoms'])==2000000000000
  assert sum(int(x['exchange_atoms']) for x in s['accounts'])==int(s['module_atoms'])
  assert sum(int(x['gas_atoms']) for x in s['accounts']+s['operator_accounts'])+int(s['gas_collector_atoms'])==6000000000
  assert all(int(x[k])>=0 for x in s['accounts'] for k in ('bank_atoms','exchange_atoms','gas_atoms','epoch','sequence'))
 def sign(name,user=0,**edit):
  snap=snapshot();ac=next(x for x in snap['accounts'] if x['account_number']==str(user))
  i={'key_index':user,'owner':ac['owner'],'sequence':ac['sequence'],'account_number':ac['account_number'],'epoch':ac['epoch'],'request_id':hashlib.sha256(name.encode()).hexdigest(),'genesis_hash':m['genesis_sha256'],**edit}
  r=subprocess.run(['node',str(Path(__file__).with_name('sign.mjs'))],input=json.dumps(i),text=True,capture_output=True,check=True)
  raw=base64.b64decode(r.stdout);(a.output/(name+'.raw')).write_bytes(raw);return raw
 def broadcast(raw):
  data=json.dumps({'jsonrpc':'2.0','id':1,'method':'broadcast_tx_commit','params':{'tx':base64.b64encode(raw).decode()}}).encode()
  with urllib.request.urlopen(urllib.request.Request(node['rpc'],data,{'Content-Type':'application/json'}),timeout=40) as r:return json.load(r)
 def case(name,success=False,user=0,raw=None,unchanged=False,**edit):
  raw=raw or sign(name,user,**edit);before=snapshot();response=broadcast(raw);after=snapshot();conserve(after)
  result=response.get('result');code=None if result is None else (result['check_tx']['code'] or result['tx_result']['code'])
  item={'name':name,'tx_hash':hashlib.sha256(raw).hexdigest().upper(),'response':response,'before':before,'after':after}
  report['cases'].append(item)
  assert result is not None,(name,response)
  assert (code==0)==success,(name,response)
  if success:assert int(result['height'])>0
  else:
   expected=EXPECTED.get(name,'EPOCH_MISMATCH' if name.startswith('stale_epoch') else 'INSUFFICIENT_CONFIRMED_BALANCE')
   assert expected in result['check_tx']['log']+result['tx_result']['log'],(name,'wrong rejection reason',response)
   for pre,post in zip(before['accounts'],after['accounts']):
    for key in ('bank_atoms','exchange_atoms','epoch'):assert pre[key]==post[key],(name,key)
  if result['check_tx']['code']==0:
   pre=next(x for x in before['accounts'] if x['account_number']==str(user));post=next(x for x in after['accounts'] if x['account_number']==str(user))
   assert int(post['sequence'])==int(pre['sequence'])+1
   assert int(post['gas_atoms'])==int(pre['gas_atoms'])-1000
  if unchanged:assert stable(before)==stable(after),(name,'unexpected mutation')
  print(name,'PASS',flush=True);return raw
 initial=snapshot();report['initial']=initial;other=next(x['owner'] for x in initial['accounts'] if x['account_number']=='1')
 case('positive_control',True,amount='1');case('positive_control_return',True,op='Withdraw',amount='1')
 for name,edit in [('wrong_chain',{'chain_id':'wrong-chain'}),('wrong_account',{'account_number':'999'}),('owner_key_mismatch',{'owner':other}),('unregistered_key',{'key_index':2,'owner':None}),('signature_tamper',{'tamper':True}),('fee_granter',{'granter':other}),('fee_payer',{'payer':other}),('wrong_fee_denom',{'fee_denom':'DEVQUOTE'}),('insufficient_fee',{'fee':'999'}),('gas_overflow',{'gas':'18446744073709551615'}),('bank_bypass',{'bank_send':True,'destination':other})]:case(name,unchanged=True,**edit)
 for name,edit in [('wrong_genesis',{'genesis_hash':'11'*32}),('zero',{'amount':'0'}),('negative',{'amount':'-1'}),('leading_zero',{'amount':'01'}),('fraction',{'amount':'1.5'}),('max_plus_one',{'amount':'1000000000001'}),('u128_overflow',{'amount':'340282366920938463463374607431768211456'}),('epoch_overflow',{'epoch':'18446744073709551616'}),('expired',{'expiry':'1'}),('overdraft',{'op':'Withdraw'})]:case(name,**edit)
 for user in range(2):
  raw=case(f'deposit{user}',True,user,amount='1000000')
  # Comet may return an RPC duplicate-cache error; verify state independently.
  before=snapshot();response=broadcast(raw);after=snapshot();assert stable(before)==stable(after);report['cases'].append({'name':f'exact_replay{user}','response':response,'before':before,'after':after})
  case(f'withdraw{user}',True,user,op='Withdraw',amount='400000')
  case(f'stale_epoch{user}',user=user,op='Withdraw',amount='1',epoch=str(int(next(x['epoch'] for x in snapshot()['accounts'] if x['account_number']==str(user)))-1))
  case(f'overdraft_after{user}',user=user,op='Withdraw',amount='600001')
 # Two pending withdrawal envelopes with the same committed sequence/epoch.
 first=sign('race_first',op='Withdraw',amount='400000');second=sign('race_second',op='Withdraw',amount='400000')
 case('race_first',True,raw=first);case('race_second',raw=second,unchanged=True)
 # Direct signed withdrawal remains possible without REST/Wallet mediation.
 case('direct_recovery',True,op='Withdraw',amount='200000')
 # Maximum canonical amount on user0 after full recovery, then insufficient bank.
 case('maximum',True,amount='1000000000000');case('empty_bank',amount='1')
 case('maximum_return',True,op='Withdraw',amount='1000000000000')
 final=snapshot();conserve(final);report['final']=final
 h=min(int(rpc(n,'/status')['sync_info']['latest_block_height']) for n in m['nodes']);blocks=[rpc(n,f'/block?height={h}') for n in m['nodes']]
 assert len({b['block_id']['hash'] for b in blocks})==1
 report['consensus']={'height':h,'blocks':blocks,'validators':rpc(node,f'/validators?height={h}')}
 assert len(report['consensus']['validators']['validators'])==4
 verify_success(report)
 report['status']='PASS'
finally:
 if proc:
  proc.terminate()
  try:proc.wait(timeout=45)
  except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
  report['cleanup']={'supervisor_returncode':proc.returncode}
 if log:log.close()
 (a.output/'result.json').write_text(json.dumps(report,indent=2)+'\n')
