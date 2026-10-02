"""S2 specification fixtures; no service, SDK state, or runtime acceptance."""
from pathlib import Path
import base64, copy, hashlib, json, struct
R=Path(__file__).resolve().parents[1]
ROOT=R.parents[1]
def write(name,value):
 p=R/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
def sha(b):return hashlib.sha256(b).hexdigest()
def canonical(o):return json.dumps(o,sort_keys=True,ensure_ascii=True,separators=(',',':')).encode()
def frame(d,b):
 d=d.encode();return struct.pack('>I',len(d))+d+struct.pack('>Q',len(b))+b
def vi(n):
 o=b''
 while n>=128:o+=bytes([(n&127)|128]);n>>=7
 return o+bytes([n])
def encode(fields):
 out=b''
 for t,k,v in fields:
  if k in ('u32','u64'):out+=vi(t*8)+vi(int(v))
  else:
   b=bytes.fromhex(v) if k=='hex' else v.encode('ascii')
   out+=vi(t*8+2)+vi(len(b))+b
 return out
profile={'schema_version':'1','profile':'s2-local-v1','test_only':True,'chain_id':'nus-s2-dev-1','market_id':'DEVBASE/DEVQUOTE','exchange_module_id':'x/exchange','market_config_version':'1','operator_epoch':'1','assets':[{'denom':d,'decimals':'6','initial_bank_atoms_per_user':'1000000000000'} for d in ['DEVBASE','DEVQUOTE']], 'gas_denom':'DEVGAS','initial_gas_atoms_per_user':'1000000000','registered_users':'2','validator_count':'4','base_atoms_per_lot':'1000','quote_atoms_per_lot_tick':'1','min_qty_lots':'1','max_qty_lots':'1000000','min_price_ticks':'1','max_price_ticks':'1000000','max_order_quote_atoms':'1000000000000','max_open_orders_per_owner':'100','max_open_orders_total':'200','fee_asset_policy_id':'RECEIVE_ASSET_V1','active_fee_version':'1','fee_profiles':[{'version':'1','bps':'0','use':'default'},{'version':'2','bps':'25','use':'separate-test-run'}],'min_expiry_delta_blocks':'2','max_expiry_delta_blocks':'1000','default_expiry_delta_blocks':'100','poll_interval_ms':'1000','rpc_timeout_ms':'2000','max_freshness_ms':'5000','max_future_block_time_ms':'1000','challenge_ttl_seconds':'120','session_ttl_seconds':'300','origins':['http://127.0.0.1:5173','http://localhost:5173'],'max_request_bytes':'16384','max_journal_payload_bytes':'16777216','max_read_items':'200','stp':'CANCEL_TAKER_REMAINDER','durability':'LOCAL_FSYNC','replicated':False,'settlement_submission_enabled':False,'wal_gc_enabled':False,'runtime_root':'.runtime/s2/','genesis_binding':'SHA256 exact runtime bytes; fixture hash forbidden'}
write('profile.json',profile)
# JSON Schema uses integer strings; exact upper bounds are additionally semantic constraints.
s={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'urn:nus:s2:1','title':'S2 service JSON schema','oneOf':[],'$defs':{}}
d=s['$defs']
for n,width in [('U32',32),('U64',64),('Atoms',128)]:d[n]={'type':'string','pattern':'^(0|[1-9][0-9]*)$','maxLength':len(str(2**width-1)),'description':f'unsigned <2^{width}; semantic bound mandatory'}
d['Hash']={'type':'string','pattern':'^[0-9a-f]{64}$'}
d['Bytes']={'type':'string','pattern':'^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$','description':'canonical RFC4648 padded base64; semantic decoded length enforced'}
d['Text']={'type':'string','minLength':1,'maxLength':128,'pattern':'^[A-Za-z0-9._:/-]+$'}
d['Bool']={'type':'boolean'}
d['NullableU64']={'anyOf':[{'$ref':'#/$defs/U64'},{'type':'null'}]}
def ref(n):return {'$ref':'#/$defs/'+n}
def enum(*v):return {'type':'string','enum':list(v)}
def arr(n,limit=200):return {'type':'array','maxItems':limit,'items':ref(n)}
def obj(n,fields):
 d[n]={'type':'object','additionalProperties':False,'required':list(fields),'properties':{k:(ref(v) if isinstance(v,str) else v) for k,v in fields.items()}}
obj('Context',{'schema_version':enum('1'),'chain_id':'Text','genesis_hash':'Hash','contract_hash':'Hash','config_hash':'Hash','market_id':'Text','market_config_version':'U64'})
obj('AssetBalance',{'denom':enum('DEVBASE','DEVQUOTE'),'bank_atoms':'Atoms','confirmed_atoms':'Atoms'})
obj('ChainAccount',{'owner':'Bytes','public_key_type':enum('ML_DSA_65'),'public_key':'Bytes','account_number':'U64','sequence':'U64','owner_epoch':'U64','gas_atoms':'Atoms','balances':arr('AssetBalance',2)})
obj('AssetSupply',{'denom':enum('DEVBASE','DEVQUOTE'),'module_atoms':'Atoms','bank_supply_atoms':'Atoms','genesis_supply_atoms':'Atoms'})
obj('Market',{'market_id':'Text','config_version':'U64','base_denom':enum('DEVBASE'),'quote_denom':enum('DEVQUOTE'),'base_atoms_per_lot':'Atoms','quote_atoms_per_lot_tick':'Atoms','min_qty_lots':'U64','max_qty_lots':'U64','min_price_ticks':'U64','max_price_ticks':'U64','max_order_quote_atoms':'Atoms','fee_policy_version':'U64','fee_bps':'U32'})
obj('ChainSnapshotBody',{'context':'Context','observed_height':'U64','block_hash':'Hash','block_time_unix_ms':'U64','market':'Market','accounts':arr('ChainAccount',2),'supplies':arr('AssetSupply',2)})
obj('ChainSnapshot',{'snapshot_id':'Hash','body':'ChainSnapshotBody'})
obj('Observation',{'snapshot_id':'Hash','observed_height':'U64','cursor_height':'U64','received_at_unix_ms':'U64','block_age_ms':'U64','query_latency_ms':'U64','last_success_age_ms':'U64','catching_up':'Bool','fresh':'Bool'})
obj('Status',{'context':'Context','stream_seq':'U64','revision':'U64','mode':enum('OPEN','CATCHING_UP','STALE','CORRECTING','WITHDRAW_FROZEN','RECOVERY_REQUIRED'),'reason':'Text','observation':'Observation','durability':enum('LOCAL_FSYNC'),'replicated':'Bool','settlement_submission_enabled':'Bool'})
obj('Network',{'context':'Context','profile':enum('s2-local-v1'),'gas_denom':enum('DEVGAS'),'assets':arr('AssetInfo',2),'origins':arr('Text',2),'status':'Status'})
obj('AssetInfo',{'denom':enum('DEVBASE','DEVQUOTE'),'decimals':'U32','atoms_per_unit':'Atoms'})
obj('SignedCommand',{'context':'Context','wire_base64':'Bytes','signature_base64':'Bytes'})
obj('CommandReceipt',{'context':'Context','kind':enum('ORDER','CANCEL','WITHDRAW_PREPARE','WITHDRAW_ABORT'),'request_id':'Hash','request_hash':'Hash','owner':'Bytes','owner_epoch':'U64','command_seq':'U64','state':enum('LOCAL_ACCEPTED','REJECTED'),'code':'Text','durability':enum('LOCAL_FSYNC'),'replicated':'Bool','observed_height':'U64','snapshot_id':'Hash','result_hash':'Hash','journal_commit_hash':'Hash'})
obj('LedgerRow',{'denom':enum('DEVBASE','DEVQUOTE'),'C':'Atoms','R':'Atoms','D':'Atoms','P':'Atoms','A':'Atoms'})
obj('OrderView',{'order_id':'Hash','order_hash':'Hash','owner_epoch':'U64','admission_seq':'U64','side':enum('BUY','SELL'),'order_type':enum('LIMIT_GTC','LIMIT_IOC'),'limit_price_ticks':'U64','max_qty_lots':'U64','remaining_qty_lots':'U64','filled_qty_lots':'U64','corrected_qty_lots':'U64','cancelled_qty_lots':'U64','state':enum('OPEN','PARTIALLY_FILLED','FILLED_PENDING','CANCELLED_OFFCHAIN','EXPIRED','STP_CANCELLED','POLICY_REJECTED_REMAINDER','CORRECTED'),'revision':'U64'})
obj('FillView',{'fill_id':'Hash','own_order_id':'Hash','command_seq':'U64','match_index':'U32','quantity_lots':'U64','execution_price_ticks':'U64','fee_policy_version':'U64','fee_base_atoms':'Atoms','fee_quote_atoms':'Atoms','state':enum('PENDING','CORRECTED'),'reason':'Text','revision':'U64'})
obj('LedgerView',{'context':'Context','owner':'Bytes','owner_epoch':'U64','stream_seq':'U64','revision':'U64','snapshot_id':'Hash','observed_height':'U64','ledger':arr('LedgerRow',2),'orders':arr('OrderView'),'fills':arr('FillView',1000),'next_cursor':'Text','status':'Status'})
obj('BookLevel',{'price_ticks':'U64','qty_lots':'U64','order_count':'U32'})
obj('BookSnapshot',{'context':'Context','stream_seq':'U64','revision':'U64','snapshot_id':'Hash','observed_height':'U64','bids':arr('BookLevel'),'asks':arr('BookLevel'),'content_hash':'Hash'})
obj('Error',{'code':'Text','retryable':'Bool','state':enum('REJECTED','SUBMISSION_UNKNOWN','NOT_FOUND_AT_SEQ'),'observed_height':'NullableU64','stream_seq':'NullableU64'})
obj('ChallengeRequest',{'owner':'Bytes','origin':'Text','audience':enum('exchange-api')})
obj('ChallengeResponse',{'wire_base64':'Bytes'})
obj('SessionResponse',{'token':'Bytes','owner':'Bytes','origin':'Text','audience':enum('exchange-api'),'genesis_hash':'Hash','expiry_time':'U64'})
obj('Correction',{'snapshot_id':'Hash','changed_owners':arr('Bytes',2),'affected_owners':arr('Bytes',2),'cancelled_order_hashes':arr('Hash'),'corrected_fill_ids':arr('Hash',1000),'reason':enum('OWNER_EPOCH_CHANGED'),'before_state_hash':'Hash','after_state_hash':'Hash'})
obj('JournalRecord',{'context':'Context','command_seq':'U64','previous_commit_hash':'Hash','command_kind':enum('ORDER','CANCEL','SNAPSHOT','EXPIRY','WITHDRAW_PREPARE','WITHDRAW_ABORT','CORRECTION'),'recorded_at_unix_ms':'U64','request_wire':'Bytes','signature':'Bytes','signature_hash':'Hash','snapshot':'ChainSnapshot','observation':'Observation','before_state_hash':'Hash','after_state_hash':'Hash','result_json':'Bytes','state_json':'Bytes','result_hash':'Hash','external_event_ids':arr('Hash',1000)})
obj('CommitMarker',{'schema_version':enum('1'),'genesis_hash':'Hash','contract_hash':'Hash','last_command_seq':'U64','record_hash':'Hash','end_offset':'U64'})
obj('LocalAction',{'request_id':'Hash'})
obj('Binding',{'owner':'Bytes','owner_epoch':'U64','kind':enum('ORDER','CANCEL','WITHDRAW_PREPARE','WITHDRAW_ABORT'),'id':'Hash','request_hash':'Hash','first_command_seq':'U64'})
obj('StoredOrder',{'owner':'Bytes','view':'OrderView','order_wire':'Bytes','signature':'Bytes'})
obj('EngineAccount',{'owner':'Bytes','owner_epoch':'U64','ledger':arr('LedgerRow',2),'withdraw_frozen':'Bool'})
obj('OutboxFill',{'fill_id':'Hash','maker_order_hash':'Hash','taker_order_hash':'Hash','buyer_order_hash':'Hash','seller_order_hash':'Hash','command_seq':'U64','match_index':'U32','quantity_lots':'U64','execution_price_ticks':'U64','fee_policy_version':'U64','fee_base_atoms':'Atoms','fee_quote_atoms':'Atoms','buy_D':'Atoms','sell_D':'Atoms','buyer_P':'Atoms','seller_P':'Atoms','snapshot_id':'Hash','state':enum('PENDING','CORRECTED'),'revision':'U64','reason':'Text','export_state':enum('HELD_S2'),'submission_enabled':'Bool'})
obj('ResultIndex',{'command_seq':'U64','request_hash':'Hash','result_hash':'Hash'})
obj('EngineState',{'context':'Context','last_command_seq':'U64','chain_snapshot':'ChainSnapshot','mode':'Text','accounts':arr('EngineAccount',2),'orders':arr('StoredOrder'),'fills':arr('OutboxFill'),'bindings':arr('Binding')})
for k in ['orders','fills','bindings']:d['EngineState']['properties'][k].pop('maxItems')
obj('LedgerChange',{'owner':'Bytes','before':'LedgerRow','after':'LedgerRow'})
obj('CommandResult',{'command_seq':'U64','kind':'Text','request_hash':'Hash','code':'Text','state':enum('LOCAL_ACCEPTED','REJECTED'),'observed_height':'U64','snapshot_id':'Hash','affected_order_hashes':arr('Hash'),'created_fill_ids':arr('Hash',1000),'corrected_fill_ids':arr('Hash',1000),'ledger_changes':arr('LedgerChange',4),'after_state_hash':'Hash'})
# root clients validate the named definition to avoid ambiguous envelopes.
s['oneOf']=[ref(n) for n in ['ChainSnapshot','Network','SignedCommand','CommandReceipt','LedgerView','BookSnapshot','Error','ChallengeRequest','ChallengeResponse','SessionResponse','Correction','JournalRecord','CommitMarker']]
write('schema.json',s)
errors=[]
for code,status,retry,state in [('RESOURCE_LIMIT','413',False,'REJECTED'),('UNAUTHORIZED','401',False,'REJECTED'),('FORBIDDEN','403',False,'REJECTED'),('ID_CONFLICT','409',False,'REJECTED'),('ORDER_NOT_FOUND','404',False,'REJECTED'),('EXPIRY_MARGIN','422',False,'REJECTED'),('OPEN_ORDER_LIMIT','422',False,'REJECTED'),('UNSETTLED_HOLD','409',False,'REJECTED'),('STALE_SNAPSHOT','503',True,'REJECTED'),('CATCHING_UP','503',True,'REJECTED'),('HEIGHT_REGRESSION','503',False,'REJECTED'),('SNAPSHOT_CONFLICT','503',False,'REJECTED'),('SNAPSHOT_UNAVAILABLE','503',True,'REJECTED'),('RECOVERY_REQUIRED','503',False,'SUBMISSION_UNKNOWN'),('WRITER_ALREADY_RUNNING','503',False,'REJECTED'),('SUBMISSION_UNKNOWN','202',True,'SUBMISSION_UNKNOWN'),('NOT_FOUND_AT_SEQ','404',True,'NOT_FOUND_AT_SEQ')]:errors.append(dict(code=code,http_status=status,retryable=retry,state=state))
write('errors.json',{'inherit':'protocol/v1/CONTRACT.md + DECISION-PORT.md + protocol/s1/CONTRACT.md','fallback_http_status':'422','new_errors':errors})
accept=[]
for i,(owner,evidence) in enumerate([('B/F/H/J','실제 두 자산 예치 TX/height, C/module/supply, genesis bytes hash'),('C/H/J','FIFO/부분체결/취소/STP/IOC Err callback 원명령과 ID/누계'),('C/D/H/J','실제 두 키 서명과 wrong domain/context/epoch/expiry/ID 재시도'),('C/H/J','C/R/D/P 전후, 가격개선·fee25·동시과예약 부정'),('B/C/D/H/J','직접 출금 TX/epoch·양측 및 후속 정정·gap/단절/역행'),('C/F/H/J','crash 지점·원본 WAL/marker·외부 ACK ledger·재생 hash·writer2 거절'),('D/E/H/J','브라우저 시연·세션/계정전환·키전송0·UNKNOWN/stale'),('F/G/J','S0/S1 회귀·문서 단독 새 환경·실측 자원/시간'),('I/J','승인 PR head/main SHA/tree·필수 CI·새 checkout 독립 QA')],1):accept.append({'id':f'S2-AT{i:02d}','owners':owner,'required_evidence':evidence,'a_stage_result':'NOT_RUN'})
write('acceptance.json',{'cases':accept,'legacy_full_pass':'0/16','not_promoted':['T04','T06','T07','T08','T09','T10','T13','T14','T16']})
# Literal economic results are kept separate from the verification formulas.
write('vectors/arithmetic.json',{'cases':[
 {'id':'demo-fill-0bps','q':'1000','execution_ticks':'10000','buy_limit_ticks':'10000','bps':'0','cap':'0','expected':{'base':'1000000','quote':'10000000','buy_D':'10000000','sell_D':'1000000','buyer_P':'1000000','seller_P':'10000000','fee_base':'0','fee_quote':'0'}},
 {'id':'price-improvement','q':'1000','execution_ticks':'9000','buy_limit_ticks':'10000','bps':'0','cap':'0','expected':{'base':'1000000','quote':'9000000','buy_D':'10000000','sell_D':'1000000','buyer_P':'1000000','seller_P':'9000000','fee_base':'0','fee_quote':'0'}},
 {'id':'fee25','q':'1000','execution_ticks':'10000','buy_limit_ticks':'10000','bps':'25','cap':'25','expected':{'base':'1000000','quote':'10000000','buy_D':'10000000','sell_D':'1000000','buyer_P':'997500','seller_P':'9975000','fee_base':'2500','fee_quote':'25000'}},
 {'id':'ceil-small','q':'1','execution_ticks':'401','buy_limit_ticks':'401','bps':'25','cap':'4294967295','expected':{'base':'1000','quote':'401','buy_D':'401','sell_D':'1000','buyer_P':'997','seller_P':'399','fee_base':'3','fee_quote':'2'}},
 {'id':'fee-ge-receive','q':'1','execution_ticks':'1','buy_limit_ticks':'1','bps':'25','cap':'25','error':'FEE_GE_RECEIVE'},
 {'id':'fee-cap','q':'1000','execution_ticks':'10000','buy_limit_ticks':'10000','bps':'25','cap':'24','error':'FEE_CAP'},
 {'id':'bps-over','q':'1','execution_ticks':'401','buy_limit_ticks':'401','bps':'10001','cap':'4294967295','error':'BPS_RANGE'}],
 'split_fee':{'receive_parts':['401','401'],'bps':'25','expected_split':'4','expected_combined':'3'}})
def row(C,Rv=0,D=0,P=0):return {k:str(v) for k,v in dict(C=C,R=Rv,D=D,P=P,A=C-Rv-D).items()}
write('vectors/ledger.json',{'unit':'atoms','scope':'synthetic expectations; runtime must obtain C by actual deposits','steps':[
 {'id':'deposited','A_BASE':row(10000000),'A_QUOTE':row(0),'B_BASE':row(0),'B_QUOTE':row(100000000)},
 {'id':'sell-2','A_BASE':row(10000000,2000000),'A_QUOTE':row(0),'B_BASE':row(0),'B_QUOTE':row(100000000)},
 {'id':'buy-1-fill','A_BASE':row(10000000,1000000,1000000),'A_QUOTE':row(0,P=10000000),'B_BASE':row(0,P=1000000),'B_QUOTE':row(100000000,D=10000000)},
 {'id':'cancel-remainder','A_BASE':row(10000000,D=1000000),'A_QUOTE':row(0,P=10000000),'B_BASE':row(0,P=1000000),'B_QUOTE':row(100000000,D=10000000)},
 {'id':'A-direct-withdraw-9-correct-component','A_BASE':row(1000000),'A_QUOTE':row(0),'B_BASE':row(0),'B_QUOTE':row(100000000)}],
 'negative_orders':[{'id':'P-is-not-available','owner_asset':'B_BASE','after':'buy-1-fill','new_reserve':'1000','expected':'INSUFFICIENT_CONFIRMED_BALANCE'},{'id':'overreserve','owner_asset':'B_QUOTE','after':'buy-1-fill','new_reserve':'90000001','expected':'INSUFFICIENT_CONFIRMED_BALANCE'}],
 'ioc':{'requested_lots':'5000','filled_lots':'3000','limit_ticks':'10000','actual_ticks':'9000','upstream_return':'Err','callback_count':'1','expected_R':'0','expected_D':'30000000','expected_P_base':'3000000','released_R':'20000000','price_improvement_held':'3000000'}})
fresh=[]
for id,change,expected in [('fresh',{},'OPEN'),('5000-inclusive',{'block_age_ms':'5000','last_success_age_ms':'5000'},'OPEN'),('block-stale',{'block_age_ms':'5001'},'STALE_SNAPSHOT'),('query-stale',{'last_success_age_ms':'5001'},'STALE_SNAPSHOT'),('catching-up',{'catching_up':True},'CATCHING_UP'),('gap',{'next_height':'102'},'CATCHING_UP'),('regression',{'next_height':'99'},'HEIGHT_REGRESSION'),('same-height-conflict',{'next_height':'100','same_snapshot':False},'SNAPSHOT_CONFLICT'),('duplicate',{'next_height':'100'},'NO_EFFECT'),('future',{'future_block_ms':'1001'},'STALE_SNAPSHOT')]:
 a={'cursor_height':'100','next_height':'101','same_snapshot':True,'block_age_ms':'1000','last_success_age_ms':'1000','future_block_ms':'0','catching_up':False};a.update(change);fresh.append({'id':id,'input':a,'expected':expected})
write('vectors/state-cases.json',{'freshness':fresh,'expiry':[{'h':'100','expiry':str(e),'expected':r} for e,r in [(100,'EXPIRED'),(101,'EXPIRY_MARGIN'),(102,'OK'),(1100,'OK'),(1101,'EXPIRY_MARGIN')]],'correction':[{'id':'both-parties','changed':['A'],'fills':[['f1','A','B']],'expected_owners':['A','B'],'expected_fills':['f1']},{'id':'transitive-isolated','changed':['A'],'fills':[['f1','A','B'],['f2','B','C'],['f3','C','D'],['f4','X','Y']],'expected_owners':['A','B','C','D'],'expected_fills':['f1','f2','f3']},{'id':'duplicate-event','changed':['A'],'fills':[],'expected_owners':['A'],'expected_fills':[]}], 'receipt':[{'id':'same-after-expiry','stored_hash':'a','request_hash':'a','expired':True,'expected':'RETURN_ORIGINAL'},{'id':'conflict','stored_hash':'a','request_hash':'b','expired':False,'expected':'ID_CONFLICT'},{'id':'new-expired','stored_hash':None,'request_hash':'a','expired':True,'expected':'EXPIRED'}]})
write('vectors/recovery.json',{'scope':'required product fault injections; specification only','cases':[{'id':k,'expected':v} for k,v in [('before-append','NO_SUCCESS_REPLAY_PREVIOUS'),('partial-header','PRESERVE_EVIDENCE_STOP'),('partial-payload','PRESERVE_EVIDENCE_STOP'),('after-wal-fsync-before-marker','UNKNOWN_TAIL_PRESERVE_AND_RECONCILE'),('after-marker-before-response','REPLAY_ONCE_RETRY_ORIGINAL_RECEIPT'),('after-response','REPLAY_ACKED_COMMAND'),('header-bitflip','RECOVERY_REQUIRED_BYTES_UNCHANGED'),('completed-payload-bitflip','RECOVERY_REQUIRED_BYTES_UNCHANGED'),('missing-marker-committed-frame','RECOVERY_REQUIRED_BYTES_UNCHANGED'),('stale-snapshot-valid-wal','PRESERVE_SNAPSHOT_FULL_WAL_REPLAY'),('both-files-truncated','EXTERNAL_ACK_LEDGER_DETECTS_LOCAL_LIMIT'),('second-writer','WRITER_ALREADY_RUNNING'),('replay-outbox','ZERO_EXTERNAL_SENDS'),('correction-mid-commit','REPLAY_SAME_COMPONENT_ONCE')]]})
# Reuse real public test key; change sign bytes and generate signatures with pinned Go tool.
legacy=json.loads((ROOT/'protocol/v1/vectors/signatures.json').read_text())
genesis_bytes=b'NUS S2 synthetic fixture genesis only; not a chain genesis\n';genesis=sha(genesis_bytes)
write('vectors/genesis-fixture.json',{'kind':'SYNTHETIC_NOT_RUNTIME','bytes_hex':genesis_bytes.hex(),'sha256':genesis})
keys=json.loads((R/'vectors/test-keys.json').read_text())
cases=[]
for p in legacy['positives']:
 p=copy.deepcopy(p);p.pop('signature_hex');fields=p['fields']
 for f in fields:
  t,k,v=f
  if t==2:f[2]='nus-s2-dev-1'
  if (p['id'] in ('order','cancel') and t==3) or (p['id']=='wallet' and t==8):f[2]=genesis
  if p['id']=='order':
   if t==5:f[2]='DEVBASE/DEVQUOTE'
   if t==6:f[2]='1'
   if t==10:f[2]='0'
   if t==11:f[2]='2'
   if t==12:f[2]='10000'
   if t==13:f[2]='2000'
   if t==14:f[2]='0'
   if t==16:f[2]='200'
   if t==17:f[2]='1'
  if p['id']=='cancel':
   if t==4:f[2]='DEVBASE/DEVQUOTE'
   if t==6:f[2]='0'
   if t==8:f[2]=cases[0]['sha256']
   if t==10:f[2]='200'
  if p['id']=='wallet':
   if t==3:f[2]='http://127.0.0.1:5173'
   if t==4:f[2]='exchange-api'
   if t==7:f[2]='1790956800'
   if t==9:f[2]='1790956680'
 b=encode(fields);f=frame(bytes.fromhex(p['domain_hex']).decode(),b)
 p['test_seed_hex']=keys[0]['test_seed_hex']
 p.update(canonical_hex=b.hex(),sign_input_hex=f.hex(),sha256=sha(f),signature_hex='')
 cases.append(p)
buyer=copy.deepcopy(cases[0]);buyer['id']='buyer-order';buyer.update(keys[1]);buyer['signature_hex']=''
for f in buyer['fields']:
 if f[0]==7:f[2]=buyer['owner_raw_hex']
 if f[0]==8:f[2]=buyer['public_key_hex']
 if f[0]==9:f[2]=sha(b'S2 buyer order 1')
 if f[0]==11:f[2]='1'
 if f[0]==13:f[2]='1000'
b=encode(buyer['fields']);fr=frame('NUS/ORDER/V1',b)
buyer.update(canonical_hex=b.hex(),sign_input_hex=fr.hex(),sha256=sha(fr));cases.append(buyer)
write('vectors/signed.json',{'profile':'s2-synthetic-crypto','test_seed_hex':legacy['test_seed_hex'],'genesis_hash':genesis,'cases':cases})
print('generated S2 schema/profile and specification fixtures; signatures require sign.go --generate')
# Canonical JSON/hash and schema boundary examples are synthetic; these hashes are not runtime manifests.
context={'schema_version':'1','chain_id':'nus-s2-dev-1','genesis_hash':genesis,'contract_hash':'11'*32,'config_hash':'22'*32,'market_id':'DEVBASE/DEVQUOTE','market_config_version':'1'}
owner=base64.b64encode(bytes.fromhex(cases[0]['owner_raw_hex'])).decode()
request={'context':context,'wire_base64':base64.b64encode(bytes.fromhex(cases[0]['canonical_hex'])).decode(),'signature_base64':base64.b64encode(bytes(3309)).decode()}
# signature here is a schema-length placeholder, never cryptographically valid.
envs=[{'id':'signed-wrapper-shape-only','schema':'SignedCommand','value':request,'expected':'VALID'}]
for id,edit in [('unknown-key',lambda v:v.update(unexpected=True)),('missing-field',lambda v:v.pop('signature_base64')),('wrong-context-version',lambda v:v['context'].update(schema_version='2')),('numeric-version',lambda v:v['context'].update(market_config_version=1)),('leading-zero',lambda v:v['context'].update(market_config_version='01')),('u64-overflow',lambda v:v['context'].update(market_config_version='18446744073709551616')),('hash-uppercase',lambda v:v['context'].update(genesis_hash='AB'*32)),('null-genesis',lambda v:v['context'].update(genesis_hash=None))]:
 v=copy.deepcopy(request);edit(v);envs.append({'id':id,'schema':'SignedCommand','value':v,'expected':'INVALID'})
for val,expected in [('0','VALID'),('340282366920938463463374607431768211455','VALID'),('340282366920938463463374607431768211456','INVALID'),('9007199254740993','VALID'),('1e6','INVALID'),('-1','INVALID'),('0.1','INVALID'),('01','INVALID'),(1,'INVALID')]:envs.append({'id':'atoms-'+str(val),'schema':'Atoms','value':val,'expected':expected})
write('vectors/envelopes.json',{'scope':'shape/bounds only, not authorization; two hash placeholders and zero signature forbidden at runtime','cases':envs})
hashes=[]
for id,domain,body in [('ledger-result','NUS/S2/RESULT/V1',{'code':'OK','command_seq':'2','buyer_P':'1000000','seller_P':'10000000'}),('empty-book','NUS/S2/BOOK/V1',{'context':context,'stream_seq':'0','revision':'0','snapshot_id':'33'*32,'observed_height':'100','bids':[],'asks':[]})]:
 b=canonical(body);hashes.append({'id':id,'domain':domain,'body':body,'canonical_hex':b.hex(),'sha256':sha(frame(domain,b))})
write('vectors/hashes.json',{'scope':'canonical JSON hash examples; result is a hash demonstration, not a CommandResult instance','cases':hashes})
payload=canonical({'fixture':'WAL_FRAME_ONLY','command_seq':'1','outbox':[]})
header=b'S2W1'+len(payload).to_bytes(4,'big')+hashlib.sha256(payload).digest();raw=header+hashlib.sha256(header).digest()+payload
write('vectors/wal.json',{'scope':'byte framing only, not a JournalRecord or runtime WAL','payload_hex':payload.hex(),'frame_hex':raw.hex(),'record_hash':sha(raw),'end_offset':str(len(raw))})

market={'market_id':'DEVBASE/DEVQUOTE','config_version':'1','base_denom':'DEVBASE','quote_denom':'DEVQUOTE','base_atoms_per_lot':'1000','quote_atoms_per_lot_tick':'1','min_qty_lots':'1','max_qty_lots':'1000000','min_price_ticks':'1','max_price_ticks':'1000000','max_order_quote_atoms':'1000000000000','fee_policy_version':'1','fee_bps':'0'}
accounts=[]
for i,k in enumerate(keys):
 balances=[]
 for asset,C in [('DEVBASE',10000000 if i==0 else 0),('DEVQUOTE',100000000 if i==1 else 0)]:balances.append({'denom':asset,'bank_atoms':str(1000000000000-C),'confirmed_atoms':str(C)})
 accounts.append({'owner':base64.b64encode(bytes.fromhex(k['owner_raw_hex'])).decode(),'public_key_type':'ML_DSA_65','public_key':base64.b64encode(bytes.fromhex(k['public_key_hex'])).decode(),'account_number':str(i),'sequence':'1','owner_epoch':'0','gas_atoms':'999999000','balances':balances})
accounts.sort(key=lambda a:base64.b64decode(a['owner']))
body={'context':context,'observed_height':'100','block_hash':'44'*32,'block_time_unix_ms':'1790956680000','market':market,'accounts':accounts,'supplies':[{'denom':asset,'module_atoms':str(C),'bank_supply_atoms':'2000000000000','genesis_supply_atoms':'2000000000000'} for asset,C in [('DEVBASE',10000000),('DEVQUOTE',100000000)]]}
snapshot={'snapshot_id':sha(frame('NUS/S2/SNAPSHOT/V1',canonical(body))),'body':body}
write('vectors/snapshot.json',{'scope':'synthetic committed-state expectation; no deposit TX evidence; contract/config placeholders forbidden at runtime','snapshot':snapshot})
envs.append({'id':'full-chain-snapshot-shape','schema':'ChainSnapshot','value':snapshot,'expected':'VALID'})
write('vectors/envelopes.json',{'scope':'shape/bounds only, not authorization; placeholder hashes/zero signatures forbidden at runtime','cases':envs})
fields=[[1,'utf8','nus-s2-dev-1'],[2,'utf8','DEVBASE/DEVQUOTE'],[3,'u64','1'],[4,'u64','102'],[5,'u32','0']]
wire=encode(fields);fid=sha(frame('NUS/FILL_ID/V1',wire))
write('vectors/fill-identity.json',{'fields':fields,'canonical_hex':wire.hex(),'fill_id':fid,'maker_order_hash':cases[0]['sha256'],'taker_order_hash':buyer['sha256'],'quantity_lots':'1000','execution_price_ticks':'10000'})
write('vectors/matching.json',{'scope':'normalized expectations, not runtime execution; each scenario starts a separate fixture','cases':[
 {'id':'price-before-time','makers':[{'id':'s1','seq':'1','p':'11000','q':'1000'},{'id':'s2','seq':'2','p':'10000','q':'1000'}],'buy_limit':'11000','buy_qty':'1000','expected_fill_makers':['s2']},
 {'id':'same-price-fifo','makers':[{'id':'s1','seq':'1','p':'10000','q':'1000'},{'id':'s2','seq':'2','p':'10000','q':'1000'}],'buy_limit':'10000','buy_qty':'1500','expected_fill_makers':['s1','s2']},
 {'id':'ioc-price-bound','makers':[{'id':'s1','seq':'1','p':'10001','q':'1000'}],'buy_limit':'10000','buy_qty':'1000','expected_fill_makers':[]}],
 'stp':{'makers':[{'id':'other','owner':'B','price':'9000','qty':'1000'},{'id':'self','owner':'A','price':'10000','qty':'1000'},{'id':'later','owner':'C','price':'10000','qty':'1000'}],'taker_owner':'A','taker_qty':'3000','taker_limit':'10000','expected_fills':['other'],'expected_cancelled_qty':'2000','expected_reason':'STP_CANCELLED'},
 'cancel_ordering':[{'first':'CANCEL','second':'MATCH','expected_fill_qty':'0','expected_R':'0','expected_D':'0'},{'first':'MATCH','second':'CANCEL','expected_fill_qty':'1000','expected_R':'0','expected_D':'1000000'}]})
