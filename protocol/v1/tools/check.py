"""Contract integrity and specification examples, not a product validator."""
from pathlib import Path
import hashlib,json,re,runpy,sys
r=Path(__file__).resolve().parents[1]
def sha(b):return hashlib.sha256(b).hexdigest()
def aggregate(files):return sha(''.join(f'{v}  {k}\n' for k,v in sorted(files.items())).encode())
files={str(p.relative_to(r)):sha(p.read_bytes()) for p in sorted(r.rglob('*')) if p.is_file() and p.name!='manifest.candidate.json' and '__pycache__' not in str(p)}
contract={k:v for k,v in files.items() if not k.startswith(('vectors/','evidence/'))}
vectors={k:v for k,v in files.items() if k.startswith('vectors/')}
manifest={'version':'1.0.0-rc2','wire_version':'1','status':'contract_review_pending; independent_final_validation_NOT_RUN','baseline_revision':'a979abc3-8c56-4f12-9ba6-55fa204d8be4','contract_sha256':aggregate(contract),'vectors_sha256':aggregate(vectors),'config_sha256':files['dev-config.json'],'files_sha256':files,'runtime':{'git_sha':None,'genesis_sha256':None,'toolchains':None,'dependency_locks':None,'active_fee_versions':None,'cross_language_crypto':'NOT_RUN','repository_ci':'NOT_RUN'}}
if sys.argv[1:]==['--write-manifest']:
 (r/'manifest.candidate.json').write_text(json.dumps(manifest,indent=2)+'\n')
else:
 assert not sys.argv[1:], 'unknown argument'
 assert json.loads((r/'manifest.candidate.json').read_text())==manifest,'manifest mismatch; review changes before regenerating'
 assert files['m0-baseline.md']=='27c9170f1643cea5bca78b5bfa7b6e62a6e6ba778006ef1944669fc357e77af8'
 runpy.run_path(str(r/'tools/check-message-codec.py'))
 cfg=json.loads((r/'dev-config.json').read_text())
 def evaluate(c):
  a=c['input'];kind=c['kind']
  if kind=='expiry':return 'OK' if int(a['height'])<int(a['expiry_height']) else 'EXPIRED'
  if kind=='atoms':return 'OK' if re.fullmatch(r'0|[1-9][0-9]*',a['value']) and int(a['value'])<2**128 else 'INTEGER_RANGE'
  if kind=='fill':
   q,p,bps=(int(a[x]) for x in ('q','p','bps'))
   if not (int(cfg['min_qty_lots'])<=q<=int(cfg['max_qty_lots']) and int(cfg['min_price_ticks'])<=p<=int(cfg['max_price_ticks'])):return 'MARKET_LIMIT'
   b=q*int(cfg['base_atoms_per_lot']);v=q*p*int(cfg['quote_atoms_per_lot_tick']);fb=(b*bps+9999)//10000;fv=(v*bps+9999)//10000
   if fb>=b or fv>=v:return 'FEE_GE_RECEIVE'
   assert max(b,v,fb,fv)<2**128
   return dict(base=str(b),quote=str(v),fee_base=str(fb),fee_quote=str(fv))
  if kind=='receipt':
   if a['stored_hash'] is not None:return 'ALREADY_COMMITTED' if a['stored_hash']==a['submitted_hash'] else 'BATCH_CONFLICT'
   seq,last=int(a['seq']),int(a['last_seq'])
   return 'RECEIPT_INCONSISTENCY' if seq<=last else 'CHECK_NEW_BATCH' if seq==last+1 else 'BATCH_SEQUENCE_GAP'
  if kind=='submission':
   assert a=={'rpc_result':'timeout','receipt_result':'NOT_FOUND_AT_HEIGHT'}
   return dict(state='SUBMISSION_UNKNOWN',release_D_P=False,new_id_allowed=False)
  if kind=='transfer':
   n,bps,cap=(int(a[x]) for x in ('amount','bps','max_fee'));f=(n*bps+9999)//10000
   if f>cap:return 'FEE_CAP'
   return dict(fee=str(f),sender_debit=str(n+f),recipient_credit=str(n))
  raise AssertionError(kind)
 cases=json.loads((r/'vectors/s0-cases.json').read_text())['cases']
 for c in cases:assert evaluate(c)==c['expected'],c['id']
 schema=json.loads((r/'schema.json').read_text());proto=(r/'protocol.proto').read_text()
 for name,fields in schema.items():
  assert 'message '+name+' {' in proto
  for f in fields:assert f" {f['name']} = {f['tag']};" in proto
 print(f'PASS {len(cases)} S0 specification examples; file integrity and schema tag mapping')
 print('contract_sha256='+manifest['contract_sha256']);print('vectors_sha256='+manifest['vectors_sha256'])
 print('NOT_RUN: generated proto compilation, product parser/crypto/state, Go/Rust/TS cross-verification, repository CI')
