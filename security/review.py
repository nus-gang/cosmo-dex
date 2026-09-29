"""Independent S0-G differential harness. Run from repository root after README setup."""
import base64,copy,hashlib,json,pathlib,subprocess,sys
ROOT=pathlib.Path(__file__).resolve().parents[1]
E=ROOT/'security/evidence'; E.mkdir(exist_ok=True)
cmds={'Go':[str(ROOT/'security/go-runner')],'Rust':[str(ROOT/'exchange/target/debug/examples/security')],'TS':['node','--experimental-strip-types',str(ROOT/'security/runner.ts')]}
ps={k:subprocess.Popen(v,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,cwd=ROOT) for k,v in cmds.items()}
rows=[]
def call(lang,r):
 p=ps[lang];p.stdin.write(json.dumps(r)+'\n');p.stdin.flush();s=p.stdout.readline()
 if not s: raise RuntimeError('runner stopped: '+lang)
 return json.loads(s)
def check(id,lang,r,expected):
 actual=call(lang,r);ok=all(actual.get(k)==v for k,v in expected.items());rows.append(dict(id=id,language=lang,expected=expected,actual=actual,passed=ok));return actual
schema=json.loads((ROOT/'protocol/v1/schema.json').read_text()); vectors=json.loads((ROOT/'protocol/v1/vectors/signatures.json').read_text())
names={'order':'OrderV1','cancel':'CancelV1','wallet':'WalletChallengeV1'};domains={'OrderV1':'NUS/ORDER/V1','CancelV1':'NUS/CANCEL/V1','WalletChallengeV1':'NUS/WALLET_AUTH/V1'}
def api(v,name):
 fields={f[0]:f[2] for f in v['fields']}
 return {f['name']:base64.b64encode(bytes.fromhex(fields[f['tag']])).decode() if f['type'] in ('a','pk','sig') else fields[f['tag']] for f in schema[name]}
for v in vectors['positives']:
 name=names[v['id']];m=api(v,name)
 for lang in ps:
  check('fixture-encode-'+v['id'],lang,dict(Op='encode',Name=name,API=m,Domain=domains[name]),dict(code='OK',wire=v['canonical_hex'],msg=v['sign_input_hex']))
  check('fixture-verify-'+v['id'],lang,dict(Op='verify',PK=v['public_key_hex'],Msg=v['sign_input_hex'],Sig=v['signature_hex']),dict(code='OK',valid=True))
for v in vectors['negatives']:
 for lang in ps:
  check('fixture-negative-'+v['id'],lang,dict(Op='verify',PK=v['public_key_hex'],Msg=v['message_hex'],Sig=v['signature_hex'],Context=v['context_hex']),dict(code='OK',valid=False))
# Distinct public synthetic seeds per producer; never a wallet/recovery secret.
generated=[]
for i,producer in enumerate(ps):
 seed=hashlib.sha256(('NUS-16 public synthetic '+producer).encode()).hexdigest()
 pk=call(producer,dict(Op='sign',Seed=seed,Msg='',Context=''))['pk']
 for v in vectors['positives']:
  name=names[v['id']];m=api(v,name);m['owner']=base64.b64encode(hashlib.sha256(bytes.fromhex(pk)).digest()[:20]).decode()
  if name=='OrderV1':m['owner_pubkey']=base64.b64encode(bytes.fromhex(pk)).decode()
  enc=call(producer,dict(Op='encode',Name=name,API=m,Domain=domains[name]));sig=call(producer,dict(Op='sign',Seed=seed,Msg=enc['msg'],Context=''))
  generated.append(dict(producer=producer,name=name,api=m,**enc,**{k:sig[k] for k in ['pk','sig']}))
  for verifier in ps:
   check('matrix-'+producer+'-'+name,verifier,dict(Op='verify',PK=pk,Msg=enc['msg'],Sig=sig['sig']),dict(code='OK',valid=True))
   check('matrix-codec-'+producer+'-'+name,verifier,dict(Op='encode',Name=name,API=m,Domain=domains[name]),dict(code='OK',wire=enc['wire'],msg=enc['msg']))
   bad=bytearray.fromhex(enc['msg']);bad[4]^=1
   check('matrix-domain-'+producer+'-'+name,verifier,dict(Op='verify',PK=pk,Msg=bad.hex(),Sig=sig['sig']),dict(code='OK',valid=False))
  ctxsig=call(producer,dict(Op='sign',Seed=seed,Msg=enc['msg'],Context='01'))
  for verifier in ps:check('matrix-context-'+producer+'-'+name,verifier,dict(Op='verify',PK=pk,Msg=enc['msg'],Sig=ctxsig['sig']),dict(code='OK',valid=False))
messages=json.loads((ROOT/'protocol/v1/vectors/message-codec.json').read_text());wires=json.loads((ROOT/'protocol/v1/vectors/wire-cases.json').read_text())
for v in messages['positives']:
 for lang in ps:check(v['id'],lang,dict(Op='encode',Name=v['message'],API=v['api_json'],Domain='NUS/PAYMENT_ID/V1'),dict(code='OK',wire=v['canonical_hex']))
for v in wires['cases']+messages['wire_cases']:
 for lang in ps:check(v['id'],lang,dict(Op='decode',Name=v['message'],Wire=v['wire_hex']),dict(code='OK' if v['expected']=='CANONICAL' else v['expected']))
# Policy uses a valid DEV-sized order, then independently signed boundary mutations.
v=vectors['positives'][0];m=api(v,'OrderV1');m.update(max_qty_lots='100',limit_price_ticks='100',max_fee_bps='25')
seed=vectors['test_seed_hex'];pk=v['public_key_hex']
policy_inputs=[]
for id,updates,extra,expected in [
 ('order-ok',{}, {},'OK'),('expiry-before',{}, {'Height':999},'OK'),('expiry-equal',{}, {'Height':1000},'EXPIRED'),('expiry-after',{}, {'Height':1001},'EXPIRED'),
 ('fee-cap-u32',{'max_fee_bps':'4294967295'},{},'OK'),
 ('fee-ge-receive',{'max_qty_lots':'1','limit_price_ticks':'1'}, {'BPS':25},'FEE_GE_RECEIVE'),
 ('owner-binding',{'owner':base64.b64encode(bytes(20)).decode()},{},'ADDRESS_MISMATCH'),
 ('registered-key-type',{}, {'RegisteredKeyType':'OTHER'},'ACCOUNT_KEY_MISMATCH'),
 ('wrong-registered-key',{}, {'RegisteredKey':'00'*1952},'ACCOUNT_KEY_MISMATCH'),
 ('wrong-chain',{}, {'Expected':{'chain_id':'other'}},'CONTEXT_MISMATCH'),
 ('u64-max-market',{'max_qty_lots':str(2**64-1)},{},'MARKET_LIMIT')]:
 a={**m,**updates};enc=call('TS',dict(Op='encode',Name='OrderV1',API=a,Domain=domains['OrderV1']));sig=call('TS',dict(Op='sign',Seed=seed,Msg=enc['msg'],Context=''))
 r=dict(Op='policy',Name='OrderV1',API=a,Wire=enc['wire'],Sig=sig['sig'],PK=pk,**extra)
 c=dict(ChainID=a['chain_id'],GenesisHash=a['genesis_hash'],ModuleID=a['exchange_module_id'],MarketID=a['market_id'],MarketConfigVersion=a['market_config_version'],RegisteredKey=base64.b64encode(bytes.fromhex(extra.get('RegisteredKey',pk))).decode(),RegisteredKeyType=extra.get('RegisteredKeyType','ML-DSA-65'),Height=extra.get('Height',999),Epoch=int(a['owner_epoch']),MaxPrice=1000000,MaxQuantity=1000000,ActiveFeeBPS=extra.get('BPS',0))
 if 'Expected' in extra:c['ChainID']=extra['Expected']['chain_id']
 r['C']=c;policy_inputs.append(dict(id=id,request=r,expected=expected))
 if id=='fee-ge-receive':
  r.update(Op='decision',SnapshotID='synthetic-1',Snapshot=dict(id='synthetic-1',source='SYNTHETIC',height='999',expiry_height=a['expiry_height'],epoch_matches=True,revoked=False,id_state='NEW',cumulative_ok=True,confirmed_balance_ok=True,q=a['max_qty_lots'],p=a['limit_price_ticks'],active_bps='25',cap=a['max_fee_bps']))
  r['C']['SnapshotID']='synthetic-1'
  for lang in ps:check(id,lang,r,dict(code='OK',decision=dict(authentication=dict(status='PASS',code='OK'),snapshot_policy=dict(status='REJECTED',code=expected,source='SYNTHETIC',snapshot_id='synthetic-1'),ack='NOT_CONNECTED',wal_replay='NOT_RUN',ledger='NOT_CONNECTED')))
 else:
  for lang in ps:check(id,lang,r,dict(code=expected))
for field,bits in [('protocol_version',32),('max_qty_lots',64)]:
 for value in [str(2**bits),'01','-1','1e0','1\n','1\r']:
  for lang in ps:check('integer-'+field+'-'+value,lang,dict(Op='encode',Name='OrderV1',API={**m,field:value},Domain=domains['OrderV1']),dict(code='INTEGER_RANGE'))
t=messages['positives'][0]
for value,code in [(str(2**128-1),'OK'),(str(2**128),'INTEGER_RANGE')]:
 for lang in ps:check('atoms-'+value,lang,dict(Op='encode',Name=t['message'],API={**t['api_json'],'amount_atoms':value},Domain='NUS/PAYMENT_ID/V1'),dict(code=code))
for receive,bps,expected in [('1000',0,dict(code='OK',fee='0')),('1000',25,dict(code='OK',fee='3')),('1',25,dict(code='FEE_GE_RECEIVE')),('0',0,dict(code='OK',fee='0')),('1000',10001,dict(code='BPS_RANGE'))]:
 for lang in ps:check('fee-'+receive+'-'+str(bps),lang,dict(Op='fee',Receive=receive,BPS=bps),expected)
for lang in ps:
 check('identifier-newline',lang,dict(Op='encode',Name='OrderV1',API={**m,'chain_id':'nus-dev-1\n'},Domain=domains['OrderV1']),dict(code='NON_CANONICAL_WIRE'))
exec((ROOT/'security/rc3_cases.py').read_text())
for p in ps.values():p.stdin.close();assert p.wait()==0
(E/'generated.json').write_text(json.dumps(generated,indent=2)+'\n')
(E/'policy-inputs.json').write_text(json.dumps(policy_inputs,indent=2)+'\n')
(E/'differential.json').write_text(json.dumps(rows,indent=2)+'\n')
summary=dict(total=len(rows),passed=sum(r['passed'] for r in rows),failures=[r for r in rows if not r['passed']])
(E/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2))
sys.exit(bool(summary['failures']))
