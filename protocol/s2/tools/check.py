"""Independent small specification oracle; never a product/runtime PASS."""
from pathlib import Path
import base64,hashlib,json,re,struct,sys
R=Path(__file__).resolve().parents[1];ROOT=R.parents[1]
def load(p):return json.loads((R/p).read_text())
def sha(b):return hashlib.sha256(b).hexdigest()
def aggregate(files):return sha(''.join(f'{v}  {k}\n' for k,v in sorted(files.items())).encode())
def canon(o):return json.dumps(o,sort_keys=True,ensure_ascii=True,separators=(',',':')).encode()
def frame(d,b):
 d=d.encode();return len(d).to_bytes(4,'big')+d+len(b).to_bytes(8,'big')+b
def hjson(d,o):return sha(frame(d,canon(o)))
INHERITED=['protocol/v1/CONTRACT.md','protocol/v1/DECISION-PORT.md','protocol/v1/adr/G-FIX-01.md','protocol/v1/schema.json','protocol/v1/protocol.proto','protocol/v1/dev-config.json','protocol/v1/manifest.candidate.json','protocol/s1/CONTRACT.md','protocol/s1/messages.proto','protocol/s1/manifest.json','chain/go.mod','chain/go.sum','chain/app/go.mod','chain/app/go.sum','exchange/Cargo.lock','web/package-lock.json']
INHERITED += [str(p.relative_to(ROOT)) for p in sorted((ROOT/'protocol/v1/vectors').glob('*')) if p.is_file()]
files={str(p.relative_to(ROOT)):sha(p.read_bytes()) for p in sorted(R.rglob('*')) if p.is_file() and p.name!='manifest.json' and '__pycache__' not in str(p)}
files.update({p:sha((ROOT/p).read_bytes()) for p in INHERITED})
manifest={'version':'s2-1.0.0-rc1','status':'Security_QA_review_pending','base_main_sha':'24029b811e5ec798bbe57f769de3d3f254c90ab7','base_tree':'52478090ff2c3225ded1be53ec22efb2a47727d2','hash_algorithm':'SHA256(sorted sha256 + two spaces + repo relative path + LF)','contract_sha256':aggregate(files),'config_sha256':files['protocol/s2/profile.json'],'vectors_sha256':aggregate({k:v for k,v in files.items() if k.startswith('protocol/s2/vectors/')}),'files_sha256':files,'runtime':{'genesis_hash':None,'code_sha':None,'binary_hash':None,'acceptance':'NOT_RUN; B-J must provide actual evidence'}}
if sys.argv[1:]==['--seal']:
 (R/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print('sealed',manifest['contract_sha256']);sys.exit()
assert not sys.argv[1:]
assert load('manifest.json')==manifest,'manifest mismatch; never reseal as a consumer'
count=0
def expect(got,want,id):
 global count
 assert got==want,(id,got,want);count+=1
cfg=load('profile.json')
old=json.loads((ROOT/'protocol/v1/dev-config.json').read_text())
for k in ['base_atoms_per_lot','quote_atoms_per_lot_tick','min_qty_lots','max_qty_lots','min_price_ticks','max_price_ticks','max_open_orders_per_owner','max_order_quote_atoms']:expect(cfg[k],old[k],k)
expect(cfg['settlement_submission_enabled'],False,'no settlement')
expect(cfg['replicated'],False,'local only')
for c in load('vectors/arithmetic.json')['cases']:
 q,p,limit,bps,cap=[int(c[k]) for k in ['q','execution_ticks','buy_limit_ticks','bps','cap']]
 if bps>10000:r='BPS_RANGE'
 elif bps>cap:r='FEE_CAP'
 else:
  base=q*1000;quote=q*p;fb=(base*bps+9999)//10000;fq=(quote*bps+9999)//10000
  if bps and (fb>=base or fq>=quote):r='FEE_GE_RECEIVE'
  else:r={k:str(v) for k,v in dict(base=base,quote=quote,buy_D=q*limit,sell_D=base,buyer_P=base-fb,seller_P=quote-fq,fee_base=fb,fee_quote=fq).items()}
 expect(r,c.get('expected',c.get('error')),c['id'])
c=load('vectors/arithmetic.json')['split_fee'];parts=list(map(int,c['receive_parts']));bps=int(c['bps'])
expect(str(sum((v*bps+9999)//10000 for v in parts)),c['expected_split'],'split')
expect(str((sum(parts)*bps+9999)//10000),c['expected_combined'],'combined')
ledger=load('vectors/ledger.json');steps={s['id']:s for s in ledger['steps']}
for s in ledger['steps']:
 for k,v in s.items():
  if k=='id':continue
  a={key:int(n) for key,n in v.items()};expect(a['C']-a['R']-a['D'],a['A'],s['id']+'/'+k);assert min(a.values())>=0
# Independent expected transitions: C stays fixed until direct withdrawal; reserve moves, P stays unusable.
a=steps['sell-2']['A_BASE'];b=steps['buy-1-fill']['A_BASE'];expect(int(a['R'])-int(b['R']),int(b['D']),'reserve to D')
for k in ['A_BASE','A_QUOTE','B_BASE','B_QUOTE']:
 before=steps['buy-1-fill'][k];after=steps['cancel-remainder'][k]
 expect((after['C'],after['D'],after['P']),(before['C'],before['D'],before['P']),'cancel-only-R/'+k)
for c in ledger['negative_orders']:
 a=int(steps[c['after']][c['owner_asset']]['A']);expect('INSUFFICIENT_CONFIRMED_BALANCE' if int(c['new_reserve'])>a else 'OK',c['expected'],c['id'])
i=ledger['ioc'];expect(int(i['filled_lots'])*int(i['limit_ticks']),int(i['expected_D']),'IOC worst D');expect((int(i['requested_lots'])-int(i['filled_lots']))*int(i['limit_ticks']),int(i['released_R']),'IOC remaining R')
cases=load('vectors/state-cases.json')
for c in cases['freshness']:
 a=c['input'];h=int(a['next_height']);last=int(a['cursor_height'])
 if h<last:r='HEIGHT_REGRESSION'
 elif h==last and not a['same_snapshot']:r='SNAPSHOT_CONFLICT'
 elif h>last+1 or a['catching_up']:r='CATCHING_UP'
 elif any(int(a[k])>5000 for k in ['block_age_ms','last_success_age_ms']) or int(a['future_block_ms'])>1000:r='STALE_SNAPSHOT'
 elif h==last:r='NO_EFFECT'
 else:r='OPEN'
 expect(r,c['expected'],c['id'])
for c in cases['expiry']:
 delta=int(c['expiry'])-int(c['h']);expect('EXPIRED' if delta<=0 else 'OK' if 2<=delta<=1000 else 'EXPIRY_MARGIN',c['expected'],'expiry')
for c in cases['correction']:
 owners=set(c['changed'])
 while True:
  before=set(owners)
  for _,a,b in c['fills']:
   if a in owners or b in owners:owners|={a,b}
  if owners==before:break
 expect(sorted(owners),c['expected_owners'],c['id']+'/owners');expect([f for f,a,b in c['fills'] if a in owners or b in owners],c['expected_fills'],c['id']+'/fills')
for c in cases['receipt']:
 r=('RETURN_ORIGINAL' if c['stored_hash']==c['request_hash'] else 'ID_CONFLICT') if c['stored_hash'] is not None else ('EXPIRED' if c['expired'] else 'CHECK_NEW')
 expect(r,c['expected'],c['id'])
# Encode canonical protobuf independently, retaining singular zero presence.
def enc(fields):
 def var(n):
  b=[]
  while n>127:b.append(n%128+128);n//=128
  return bytes(b+[n])
 result=b''
 for tag,kind,value in fields:
  if kind in ['u32','u64']:
   assert 0<=int(value)<2**int(kind[1:]);result+=var(tag*8)+var(int(value))
  else:
   raw=bytes.fromhex(value) if kind=='hex' else value.encode('ascii');result+=var(tag*8+2)+var(len(raw))+raw
 return result
signed=load('vectors/signed.json')
for c in signed['cases']:
 b=enc(c['fields']);f=frame(bytes.fromhex(c['domain_hex']).decode(),b)
 expect(b.hex(),c['canonical_hex'],c['id']+'/wire');expect(f.hex(),c['sign_input_hex'],c['id']+'/frame');expect(sha(f),c['sha256'],c['id']+'/hash')
 expect(len(bytes.fromhex(c['signature_hex'])),3309,'sig length');pk=bytes.fromhex(c['public_key_hex']);expect(len(pk),1952,'pk');expect(sha(pk)[:40],c['owner_raw_hex'],'address')
fill=load('vectors/fill-identity.json');raw=enc(fill['fields']);expect(raw.hex(),fill['canonical_hex'],'fill wire');expect(sha(frame('NUS/FILL_ID/V1',raw)),fill['fill_id'],'fill ID')
matching=load('vectors/matching.json')
for c in matching['cases']:
 remaining=int(c['buy_qty']);fills=[]
 for maker in sorted(c['makers'],key=lambda m:(int(m['p']),int(m['seq']))):
  if int(maker['p'])>int(c['buy_limit']) or remaining==0:break
  amount=min(remaining,int(maker['q']));remaining-=amount
  if amount:fills.append(maker['id'])
 expect(fills,c['expected_fill_makers'],c['id'])
c=matching['stp'];remaining=int(c['taker_qty']);fills=[]
for maker in c['makers']:
 if maker['owner']==c['taker_owner']:break
 if int(maker['price'])<=int(c['taker_limit']):fills.append(maker['id']);remaining-=int(maker['qty'])
expect(fills,c['expected_fills'],'STP filled');expect(str(remaining),c['expected_cancelled_qty'],'STP remainder')
# Parse-only schema subset for our fixture envelopes; not a production validator.
schema=load('schema.json');defs=schema['$defs']
def valid(spec,v):
 if '$ref' in spec:
  name=spec['$ref'].split('/')[-1];assert name in defs;valid(defs[name],v)
  if name in ['U32','U64','Atoms']:assert int(v)<2**{'U32':32,'U64':64,'Atoms':128}[name]
  if name=='Bytes':assert base64.b64encode(base64.b64decode(v,validate=True)).decode()==v
  return
 if 'anyOf' in spec:
  ok=False
  for sub in spec['anyOf']:
   try:valid(sub,v);ok=True
   except (AssertionError,ValueError):pass
  assert ok;return
 ty=spec['type'];assert {'object':lambda:isinstance(v,dict),'array':lambda:isinstance(v,list),'string':lambda:isinstance(v,str),'boolean':lambda:type(v)==bool,'null':lambda:v is None}[ty]()
 if ty=='object':
  assert set(v)==set(spec['required'])
  for k,val in v.items():valid(spec['properties'][k],val)
 if ty=='array':
  assert len(v)<=spec.get('maxItems',2**63)
  for val in v:valid(spec['items'],val)
 if ty=='string':
  assert spec.get('minLength',0)<=len(v)<=spec.get('maxLength',2**63)
  if 'pattern' in spec:assert re.fullmatch(spec['pattern'],v)
 if 'enum' in spec:assert v in spec['enum']
# Every ref resolves; required/property completeness, no permissive objects.
def walk(v):
 if isinstance(v,dict):
  if '$ref' in v:assert v['$ref'].split('/')[-1] in defs
  if v.get('type')=='object':assert set(v['properties'])==set(v['required']) and v['additionalProperties'] is False
  for val in v.values():walk(val)
 elif isinstance(v,list):
  for val in v:walk(val)
walk(schema)
for c in load('vectors/envelopes.json')['cases']:
 try:valid({'$ref':'#/$defs/'+c['schema']},c['value']);r='VALID'
 except (AssertionError,ValueError):r='INVALID'
 expect(r,c['expected'],c['id'])
for c in load('vectors/hashes.json')['cases']:
 expect(canon(c['body']).hex(),c['canonical_hex'],c['id']+'/json');expect(hjson(c['domain'],c['body']),c['sha256'],c['id']+'/hash')
snap=load('vectors/snapshot.json')['snapshot'];expect(hjson('NUS/S2/SNAPSHOT/V1',snap['body']),snap['snapshot_id'],'snapshot hash')
accounts=snap['body']['accounts'];expect(len({a['owner'] for a in accounts}),2,'two unique owners')
for supply in snap['body']['supplies']:
 balances=[b for a in accounts for b in a['balances'] if b['denom']==supply['denom']]
 expect(sum(int(b['confirmed_atoms']) for b in balances),int(supply['module_atoms']),'module conservation')
 expect(sum(int(b['bank_atoms']) for b in balances)+int(supply['module_atoms']),int(supply['genesis_supply_atoms']),'asset supply')
# WAL byte fixture independently validates length/header/payload hashes and marker binding.
w=load('vectors/wal.json');raw=bytes.fromhex(w['frame_hex']);payload=bytes.fromhex(w['payload_hex'])
expect(raw[:4],b'S2W1','wal magic');expect(int.from_bytes(raw[4:8],'big'),len(payload),'wal length');expect(raw[8:40].hex(),sha(payload),'wal payload sha');expect(raw[40:72].hex(),sha(raw[:40]),'wal header sha');expect(raw[72:],payload,'wal payload');expect(sha(raw),w['record_hash'],'wal record hash')
for pos in range(72):
 bad=bytearray(raw);bad[pos]^=1
 assert sha(bad[:40])!=bad[40:72].hex();count+=1
expect(len(load('acceptance.json')['cases']),9,'AT retained')
expect(load('acceptance.json')['legacy_full_pass'],'0/16','legacy retained')
print(f'PASS {count} S2 specification checks; all manifest files verified')
print('contract_sha256='+manifest['contract_sha256'])
print('NOT_RUN: product Go/Rust/TS cross-verification, real deposits, API/engine/WAL crash, browser, S2-AT01..09')
