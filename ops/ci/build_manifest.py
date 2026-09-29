"""Rebuild reviewed SRE manifest from pinned source oracles, never from runner output."""
import base64, hashlib, json
from pathlib import Path
R=Path(__file__).resolve().parents[2]
def read(p): return json.loads((R/p).read_text())
def digest(p): return hashlib.sha256((R/p).read_bytes()).hexdigest()
cases=[]
def add(id,request,expected,lanes=('go','rust','ts'),scope='library'):
 cases.append(dict(id=id,request=request,expected=expected,lanes=list(lanes),scope=scope))
sig=read('protocol/v1/vectors/signatures.json');schema=read('protocol/v1/schema.json')
for c in sig['positives']:
 name={'order':'OrderV1','cancel':'CancelV1','wallet':'WalletChallengeV1'}[c['id']]
 fields={x[0]:x[2] for x in c['fields']}
 api={f['name']:base64.b64encode(bytes.fromhex(fields[f['tag']])).decode() if f['type'] in ('a','pk','sig') else fields[f['tag']] for f in schema[name]}
 add('signature/encode/'+c['id'],dict(op='encode',message=name,api=api),c['canonical_hex'])
 add('signature/frame/'+c['id'],dict(op='frame',domain=bytes.fromhex(c['domain_hex']).decode(),wire=c['canonical_hex']),c['sign_input_hex'])
 add('signature/crypto/'+c['id'],dict(op='crypto',pk=c['public_key_hex'],input=c['sign_input_hex'],signature=c['signature_hex']),True,scope='actual_crypto')
for c in sig['negatives']:
 add('signature/crypto/'+c['id'],dict(op='crypto',pk=c['public_key_hex'],input=c['message_hex'],signature=c['signature_hex'],context=c['context_hex']),c['expected_crypto_valid'],scope='actual_crypto_context_negative')
for file,key in [('wire-cases','cases'),('message-codec','wire_cases')]:
 for c in read('protocol/v1/vectors/'+file+'.json')[key]:
  add(file+'/wire/'+c['id'],dict(op='decode',message=c['message'],wire=c['wire_hex']),c['expected'])
for c in read('protocol/v1/vectors/message-codec.json')['positives']:
 add('message/encode/'+c['id'],dict(op='encode',message=c['message'],api=c['api_json']),c['canonical_hex'])
for i,c in enumerate(read('protocol/v1/vectors/amount-codec.json')['cases']):
 add('amount/'+str(i),dict(op='atoms',api=c['api_json']) if 'api_json' in c else dict(op='atoms_decode',wire=c['wire_hex']),c['wire_hex'] if c['expected']=='OK' else c['expected'])
dec=read('protocol/v1/vectors/decision-port.json')
for c in dec['fee_cases']:add('fee/'+c['id'],dict(op='fee',receive=c['receive'],rate=c['active_bps']),c['expected'])
for c in dec['cap_cases']:add('cap/'+c['id'],dict(op='cap',cap=c['cap'],rate=c['active_bps']),c['expected'])
for c in dec['decision_cases']:add('decision/'+c['id'],dict(op='decision',auth=c['input']['authentication_result'],snapshot=c['input'].get('snapshot')),c['expected'],scope='synthetic_policy_auth_injected')
for c in dec['api_errors']:add('api/'+c['code'],dict(op='api',code=c['code']),c['expected'],('rust','ts'),scope='api_mapping')
lock=read('ops/ci/input-lock.json')
mapping=[]
for rec in read('ops/ci/required-cases.json'):
 source=rec['source'];pointer=rec['json_pointer'];category=pointer.split('/')[1]
 scope='component_assertions'
 commands=['component-tests']
 if source.endswith('batches.json') or category=='state_cases':scope='reference_only';commands=['protocol-reference']
 if category=='decision_cases':scope='synthetic_policy_auth_injected';commands=['oracle-ports']
 if category in ('fee_cases','cap_cases','positives','negatives','wire_cases') or source.endswith('wire-cases.json'):commands=['oracle-ports','component-tests']
 if source.endswith('snapshot-output.json'):scope='actual_crypto_synthetic_snapshot';commands=['rc4-full-output']
 if category=='registration_cases':scope='component_actual_auth_tests';commands=['component-tests']
 mapping.append(dict(source=source,json_pointer=pointer,id=rec['id'],scope=scope,commands=commands))
c=dict(schema_version=2,contract_revision=lock['inputs']['A']['commit'],contract_version=lock['canonical_protocol']['version'],
 contract_sha256=lock['inputs']['A']['contract_sha256'],protocol_vectors_sha256=lock['inputs']['A']['vectors_sha256'],
 vectors='protocol/v1/vectors/signatures.json',vectors_sha256=digest('protocol/v1/vectors/signatures.json'),
 toolchains={'go':'1.24.4','rust':'1.92.0','node':'24.21.0','python':'3 (actual version recorded at execution)'},common_input_manifest=lock['common_input_manifest'],source_inputs=lock['inputs'],e_compatibility=lock['E_rc4_compatibility'],cases=cases,coverage=mapping,
 integer_override={'id':'I20','original':'FEE_GE_RECEIVE','rc3':'0','reason':'rc3 fee(0,0)=0; original TSV remains unchanged'},
 boundaries={'ack':'NOT_CONNECTED','ledger':'NOT_CONNECTED','wal_replay':'NOT_RUN','chain_runtime':'NOT_CONNECTED'},
 lanes={
 'go':dict(cwd='chain',build=['go','build','-mod=readonly','./...'],test=['go','test','-mod=readonly','-count=1','-v','./...'],vectors=['go','run','-mod=readonly','../ops/ci/ports/go.go']),
 'rust':dict(cwd='exchange',build=['cargo','build','--locked'],test=['cargo','test','--locked','--','--nocapture'],vectors=['cargo','run','--quiet','--locked','--manifest-path','../ops/ci/ports/rust/Cargo.toml']),
 'ts':dict(cwd='web',build=['npm','run','build'],test=['npm','test'],vectors=['node','--experimental-strip-types','../ops/ci/ports/ts.ts'])})
files={}
for folder in ['protocol/v1','chain','exchange','web','settlement']:
 for p in sorted((R/folder).rglob('*')):
  if p.is_file() and not any(x in p.parts for x in ['evidence','node_modules','target','dist','__pycache__']) and p.name!='.DS_Store':files[str(p.relative_to(R))]=digest(p.relative_to(R))
for p in ['ops/ci/rc4.py','ops/ci/vectors.py','ops/ci/build_manifest.py','ops/ci/cto-input.json','ops/ci/input-lock.json','ops/ci/required-cases.json','ops/ci/ports/go.go','ops/ci/ports/ts.ts','ops/ci/ports/rust/Cargo.toml','ops/ci/ports/rust/Cargo.lock','ops/ci/ports/rust/src/main.rs']:
 if (R/p).exists():files[p]=digest(p)
c['files_sha256']=files
import sys
if '--check' in sys.argv:
 assert read('ops/ci/manifest.json')==c, 'manifest oracle/source mapping drift'
else:
 (R/'ops/ci/manifest.json').write_text(json.dumps(c,indent=2)+'\n')
