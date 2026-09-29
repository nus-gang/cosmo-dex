"""Executable specification examples; no crypto, application, WAL or ledger."""
import json,re
from pathlib import Path
r=Path(__file__).resolve().parents[1]
def uint(s,bits):
 if not isinstance(s,str) or not re.fullmatch(r'0|[1-9][0-9]*',s) or int(s)>=2**bits: raise ValueError('INTEGER_RANGE')
 return int(s)
def bps(s):
 try: n=uint(s,32)
 except ValueError: raise ValueError('BPS_RANGE')
 if n>10000: raise ValueError('BPS_RANGE')
 return n
def fee(receive,rate):
 n=uint(receive,128); b=bps(rate)
 if b==0:return '0'
 f=(n*b+9999)//10000
 if f>=n:raise ValueError('FEE_GE_RECEIVE')
 return str(f)
def cap_check(cap,rate):
 c=uint(cap,32);b=bps(rate)
 if b>c:raise ValueError('FEE_CAP')
 return 'OK'
def outcome(fn,*args):
 try:return fn(*args)
 except ValueError as e:return str(e)
def policy(a):
 for k in ('height','expiry_height','q','p'):uint(a[k],64)
 uint(a['cap'],32)
 if a['id_state']=='CONFLICT':return 'ID_CONFLICT'
 if not a['epoch_matches']:return 'EPOCH_MISMATCH'
 if a['revoked']:return 'ORDER_REVOKED'
 if int(a['height'])>=int(a['expiry_height']):return 'EXPIRED'
 cfg=json.loads((r/'dev-config.json').read_text())
 if not all(int(cfg['min_'+k])<=int(a[x])<=int(cfg['max_'+k]) for k,x in [('qty_lots','q'),('price_ticks','p')]):return 'MARKET_LIMIT'
 cap_check(a['cap'],a['active_bps'])
 fee(str(int(a['q'])*1000),a['active_bps']);fee(str(int(a['q'])*int(a['p'])),a['active_bps'])
 if not a['cumulative_ok']:return 'CUMULATIVE_QTY_EXCEEDED'
 if not a['confirmed_balance_ok']:return 'INSUFFICIENT_CONFIRMED_BALANCE'
 return 'OK'
required={'id','source','height','expiry_height','epoch_matches','revoked','id_state','cumulative_ok','confirmed_balance_ok','q','p','active_bps','cap'}
def decision(a):
 auth=a['authentication_result'];snap=a['snapshot']
 out={'authentication':auth,'snapshot_policy':{'status':'NOT_RUN','code':None,'source':'SYNTHETIC','snapshot_id':snap.get('id') if snap else None},'ack':'NOT_CONNECTED','wal_replay':'NOT_RUN','ledger':'NOT_CONNECTED'}
 if auth['status']!='PASS':return out
 if snap is None or not required<=snap.keys() or any(snap[k] is None for k in required):
  out['snapshot_policy']['status']='NOT_CONNECTED';return out
 assert snap['source']=='SYNTHETIC'
 assert snap['id_state'] in ('NEW','CONFLICT')
 assert all(type(snap[k]) is bool for k in ('epoch_matches','revoked','cumulative_ok','confirmed_balance_ok'))
 code=outcome(policy,snap)
 out['snapshot_policy'].update(status='PASS' if code=='OK' else 'REJECTED',code=code)
 return out
v=json.loads((r/'vectors/decision-port.json').read_text())
for c in v['fee_cases']:assert outcome(fee,c['receive'],c['active_bps'])==c['expected'],c['id']
for c in v['cap_cases']:assert outcome(cap_check,c['cap'],c['active_bps'])==c['expected'],c['id']
for c in v['decision_cases']:assert decision(c['input'])==c['expected'],c['id']
for c in v['api_errors']:
 assert c['expected']=={'http_status':400,'body':{'code':c['code'],'retryable':False,'state':'REJECTED','height':None}}
print('PASS rc3 specification: fee=%d cap=%d decision=%d API=%d; crypto/application NOT_RUN'%tuple(len(v[k]) for k in ('fee_cases','cap_cases','decision_cases','api_errors')))

keys=json.loads((r/'vectors/signatures.json').read_text())['positives']
for c in v['registration_cases']:
 fixture=next(x for x in keys if x['id']==c['submitted_fixture']['positive_id'])
 key=bytes.fromhex(fixture['public_key_hex']);assert len(key)==1952
 reg=c['registered']
 if reg is None:code='ACCOUNT_KEY_UNREGISTERED'
 elif 'key_type' not in reg:code='NOT_CONNECTED'
 else:
  raw=key if reg['raw_key_ref']=='submitted' else bytes([key[0]^1])+key[1:]
  code='OK' if reg['key_type']=='ML-DSA-65' and raw==key else 'ACCOUNT_KEY_MISMATCH'
 assert code==c['expected_registration'],c['id']
print('PASS 5 registration transport examples; actual crypto NOT_RUN')
