# Executed inside review.py so these cases share the real ML-DSA runners.
dp=json.loads((ROOT/'protocol/v1/vectors/decision-port.json').read_text())
for v in dp['fee_cases']:
 expected=dict(code='OK',fee=v['expected']) if v['expected'].isdigit() else dict(code=v['expected'])
 for lang in ps:check('rc3-'+v['id'],lang,dict(Op='fee-decimal',Receive=v['receive'],Rate=v['active_bps']),expected)
for v in dp['cap_cases']:
 for lang in ps:check('rc3-'+v['id'],lang,dict(Op='cap',Cap=v['cap'],Rate=v['active_bps']),dict(code=v['expected']))
for v in dp['decision_cases']:
 for lang in ps:check('rc3-synthetic-'+v['id'],lang,dict(Op='snapshot',Auth=v['input']['authentication_result'],Snapshot=v['input'].get('snapshot')),dict(code='OK',decision=v['expected']))
# Independent signed integration cases: change the bytes, re-sign, then bind trusted context.
base_snapshot=copy.deepcopy(dp['decision_cases'][0]['input']['snapshot'])
v=vectors['positives'][0];base_order=api(v,'OrderV1');base_order.update(max_qty_lots='100',limit_price_ticks='100',max_fee_bps='25')
seed=vectors['test_seed_hex'];pk=v['public_key_hex']
def signed_request(updates=None,extra=None,snapshot_updates=None):
 a={**base_order,**(updates or {})};extra=extra or {}
 enc=call('TS',dict(Op='encode',Name='OrderV1',API=a,Domain=domains['OrderV1']));assert enc['code']=='OK',enc
 sig=call('TS',dict(Op='sign',Seed=seed,Msg=enc['msg'],Context=''))
 snap={**base_snapshot,'q':a['max_qty_lots'],'p':a['limit_price_ticks'],'cap':a['max_fee_bps'],'expiry_height':a['expiry_height'],**(snapshot_updates or {})}
 r=dict(Op='decision',Name='OrderV1',API=a,Wire=enc['wire'],Sig=sig['sig'],PK=pk,SnapshotID='synthetic-1',Height=999);r.update(extra)
 c=dict(SnapshotID=r['SnapshotID'],ChainID=a['chain_id'],GenesisHash=a['genesis_hash'],ModuleID=a['exchange_module_id'],MarketID=a['market_id'],MarketConfigVersion=a['market_config_version'],RegisteredKey=base64.b64encode(bytes.fromhex(extra.get('RegisteredKey',pk))).decode(),RegisteredKeyType=extra.get('RegisteredKeyType','ML-DSA-65'),Height=r['Height'],Epoch=extra.get('Epoch',int(a['owner_epoch'])))
 if extra.get('Unregistered'):c['RegisteredKey']=None
 if extra.get('MissingKeyType'):c['RegisteredKeyType']=''
 if extra.get('BadSig'):r['Sig']=('01' if r['Sig'][:2]!='01' else '02')+r['Sig'][2:]
 r.update(C=c,Snapshot=snap)
 return r

def expect_decision(auth='OK',policy='OK',sid='synthetic-1'):
 def stage(code):return dict(status='PASS' if code=='OK' else code if code in ('NOT_RUN','NOT_CONNECTED') else 'REJECTED',code=None if code in ('NOT_RUN','NOT_CONNECTED') else code)
 return dict(code='OK',decision=dict(authentication=stage(auth),snapshot_policy={**stage(policy),'source':'SYNTHETIC','snapshot_id':sid},ack='NOT_CONNECTED',wal_replay='NOT_RUN',ledger='NOT_CONNECTED'))
cases=[('registered-normal',{}, {},{},'OK','OK'),('registered-other',{}, {'RegisteredKeyType':'OTHER'}, {},'ACCOUNT_KEY_MISMATCH','NOT_RUN'),('registered-absent',{}, {'Unregistered':True},{},'ACCOUNT_KEY_UNREGISTERED','NOT_RUN'),('registered-missing-type',{}, {'MissingKeyType':True},{},'NOT_CONNECTED','NOT_RUN'),('registered-different',{}, {'RegisteredKey':'00'*1952},{},'ACCOUNT_KEY_MISMATCH','NOT_RUN'),('bad-signature',{}, {'BadSig':True},{},'INVALID_SIGNATURE','NOT_RUN'),('revoked',{}, {},{'revoked':True},'OK','ORDER_REVOKED'),('id-conflict',{}, {},{'id_state':'CONFLICT'},'OK','ID_CONFLICT'),('no-confirmed-balance',{}, {},{'confirmed_balance_ok':False},'OK','INSUFFICIENT_CONFIRMED_BALANCE'),('overfill',{}, {},{'cumulative_ok':False},'OK','CUMULATIVE_QTY_EXCEEDED'),('epoch-mismatch',{}, {'Epoch':999},{'epoch_matches':False},'OK','EPOCH_MISMATCH'),('expiry-equal',{}, {'Height':1000},{'height':'1000'},'OK','EXPIRED'),('fee-q-p-1',{'max_qty_lots':'1','limit_price_ticks':'1'}, {},{},'OK','FEE_GE_RECEIVE'),('side-invalid',{'side':'3'}, {},{},'OK','MARKET_LIMIT'),('order-type-invalid',{'order_type':'3'}, {},{},'OK','MARKET_LIMIT')]
for cap in ['0','25','10000','10001','4294967295']:
 cases.append(('signed-cap-'+cap,{'max_fee_bps':cap},{},{},'OK','FEE_CAP' if cap=='0' else 'OK'))
for id,updates,extra,snap,auth,policy in cases:
 r=signed_request(updates,extra,snap);policy_inputs.append(dict(id='rc3-signed-'+id,request=r,expected=expect_decision(auth,policy)))
 for lang in ps:check('rc3-signed-'+id,lang,r,expect_decision(auth,policy))
for field in base_snapshot:
 r=signed_request();r['Snapshot'].pop(field)
 for lang in ps:check('rc3-signed-missing-'+field,lang,r,expect_decision(policy='NOT_CONNECTED',sid=None if field=='id' else 'synthetic-1'))
# Missing or contradictory binding must never yield a policy PASS. Contract does not
# standardize the rejection code for contradictory snapshots; compare the invariant.
for field,value in [('id','other'),('height','998'),('q','99'),('p','99'),('cap','24'),('expiry_height','2000'),('epoch_matches',False)]:
 r=signed_request(snapshot_updates={field:value});policy_inputs.append(dict(id='binding-'+field,request=r,expected='policy must not PASS'))
 for lang in ps:
  actual=call(lang,r);d=actual.get('decision',{});ok=d.get('authentication',{}).get('status')=='PASS' and d.get('snapshot_policy',{}).get('status') in ('REJECTED','NOT_CONNECTED') and d.get('ack')=='NOT_CONNECTED'
  rows.append(dict(id='rc3-binding-'+field,language=lang,expected={'invariant':'auth PASS, policy REJECTED/NOT_CONNECTED, ACK NOT_CONNECTED'},actual=actual,passed=ok))

# A stale trusted epoch cannot be masked by a fabricated epoch_matches=true flag.
r=signed_request(extra={'Epoch':999});policy_inputs.append(dict(id='rc3-epoch-flag-binding',request=r,expected='policy must not PASS'))
for lang in ps:
 actual=call(lang,r);d=actual.get('decision',{});ok=d.get('authentication',{}).get('status')=='PASS' and d.get('snapshot_policy',{}).get('status') in ('REJECTED','NOT_CONNECTED')
 rows.append(dict(id='rc3-epoch-flag-binding',language=lang,expected={'invariant':'stale epoch must not produce policy PASS'},actual=actual,passed=ok))
