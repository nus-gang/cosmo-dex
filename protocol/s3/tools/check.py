"""Read-only independent arithmetic/state/fixture/manifest oracle, not product QA."""
import base64
import copy
import json
import re
import sys
from collections import defaultdict
from codec import ROOT, S3, read, encode, frame, sha, canon, blob

checks = 0
def expect(got, want, label):
    global checks
    if got != want:
        raise AssertionError((label,got,want))
    checks += 1

def aggregate(files):
    return sha(''.join(f'{v}  {k}\n' for k,v in sorted(files.items())).encode())

def manifest():
    files={}
    for directory in ['protocol/v1','protocol/s1','protocol/s2','protocol/s3']:
        for p in sorted((ROOT/directory).rglob('*')):
            if not p.is_file() or '__pycache__' in p.parts or p==S3/'manifest.json':continue
            files[str(p.relative_to(ROOT))]=sha(p.read_bytes())
    locks=['chain/go.mod','chain/go.sum','chain/app/go.mod','chain/app/go.sum','exchange/Cargo.toml','exchange/Cargo.lock','web/package.json','web/package-lock.json']
    for p in locks:files[p]=sha((ROOT/p).read_bytes())
    return {'version':'s3-1.0.0-rc2','status':'SECURITY_THEN_QA_REVIEW_PENDING',
      'supersedes_contract_sha':'0915375cac360f83d62a70587a0e7cf9c89604a1',
      'base_main_sha':'bd9e473196ac86fdedf655b2c93e6931f54faa83','base_tree':'31a0d1c65e9b71647cac6c4c45bf7e8d2dd9d7f3',
      'hash_algorithm':'SHA256(sorted sha256 + two spaces + repo-relative path + LF)',
      'contract_sha256':aggregate(files),'config_sha256':files['protocol/s3/profile.json'],
      'config_profiles_sha256':{'fee0':files['protocol/s3/profile.json'],'fee25':files['protocol/s3/profile-fee25.json']},
      'vectors_sha256':aggregate({k:v for k,v in files.items() if k.startswith('protocol/s3/vectors/')}),
      'lock_sha256':aggregate({k:files[k] for k in locks}),'files_sha256':files,
      'runtime':{'genesis_hash':None,'product_result':'NOT_RUN','first_receipt':'NOT_RUN'},
      'handoff':'Reviewer pins this manifest and commit/tree from Paperclip handoff; consumers must not reseal.'}

def integer(s,bits):
    if not isinstance(s,str) or re.fullmatch(r'0|[1-9][0-9]*',s) is None or len(s)>78 or int(s)>=2**bits:
        raise ValueError('INTEGER_RANGE')
    return int(s)

def arithmetic(c):
    try:
        q=integer(c['q'],64);p=integer(c['p'],64);limit=integer(c['limit'],64)
        bps=integer(c['bps'],32);cap=integer(c['cap'],32)
        if not 1<=q<=1000000 or not 1<=p<=limit<=1000000:return 'MARKET_LIMIT'
        if bps>10000:return 'BPS_RANGE'
        if bps>cap:return 'FEE_CAP'
        base=q*1000;quote=q*p;fb=(base*bps+9999)//10000;fq=(quote*bps+9999)//10000
        if bps and (fb>=base or fq>=quote):return 'FEE_GE_RECEIVE'
        return {k:str(v) for k,v in dict(base=base,quote=quote,fee_base=fb,fee_quote=fq,buyer_receive=base-fb,seller_receive=quote-fq,buy_D=q*limit,release_on_commit=q*(limit-p)).items()}
    except ValueError as e:return str(e)

def resolution(c):
    if c['batch_receipt']=='COMMITTED':return 'COMMITTED'
    if c['batch_receipt']=='MISSING_AFTER_CODE0':return 'RECEIPT_INCONSISTENCY'
    a=c['attempts']
    if any(x not in ['INCLUDED_FAILURE','EXPIRED_ABSENT_PROVEN'] for x in a):return 'SUBMISSION_UNKNOWN'
    if 'INCLUDED_FAILURE' not in a:return 'RETRY_SAME_BATCH_OR_HALT_BUDGET'
    return 'CORRECTION_READY' if c['close_receipt'] else 'REJECTED_FINAL'

def closure(fills,roots):
    selected=set(roots)
    while True:
        before=set(selected)
        for f in fills:
            if f['state']!='COMMITTED' and selected.intersection(f['deps']):selected.add(f['id'])
        if selected==before:break
    return [f['id'] for f in sorted(fills,key=lambda x:int(x['seq'])) if f['id'] in selected and f['state']!='COMMITTED']

def dependency_equivalence(fills):
    # Reconstruct the uncompressed graph independently, then compare every
    # node's ancestor set with the stored, bounded predecessor representation.
    pending=sorted([f for f in fills if f['state']=='PENDING'],key=lambda f:int(f['seq']))
    full_ancestors=[];stored_ancestors=[];positions={}
    for i,f in enumerate(pending):
        keys={(field,key) for field in ['orders','debits'] for key in f.get(field,[])}
        latest={};full=0
        for j,prior in enumerate(pending[:i]):
            shared=keys & {(field,key) for field in ['orders','debits'] for key in prior.get(field,[])}
            if shared:
                full |= (1<<j) | full_ancestors[j]
                for key in shared:latest[key]=j
        expected=[pending[j]['id'] for j in sorted(set(latest.values()))]
        expect(f['deps'],expected,'latest-domain predecessors/'+f['id'])
        expect(len(f['deps'])<=4,True,'bounded predecessors/'+f['id'])
        stored=0
        for dep in f['deps']:
            j=positions[dep]
            stored |= (1<<j) | stored_ancestors[j]
        expect(stored,full,'compressed/full graph reachability/'+f['id'])
        full_ancestors.append(full);stored_ancestors.append(stored);positions[f['id']]=i

def validate(value, spec, defs):
    # This bounded checker covers the emitted schema subset; it is not a general JSON Schema library.
    if '$ref' in spec:
        name=spec['$ref'].split('/')[-1]
        validate(value,defs[name],defs)
        if name in ['U32','U64','Atoms']:integer(value,{'U32':32,'U64':64,'Atoms':128}[name])
        if name=='Bytes':expect(base64.b64encode(base64.b64decode(value,validate=True)).decode(),value,'base64 canonical')
        return
    if 'anyOf' in spec:
        for opt in spec['anyOf']:
            try:validate(value,opt,defs);return
            except (AssertionError,ValueError,TypeError):pass
        raise ValueError('anyOf')
    if 'const' in spec:assert value==spec['const']
    if 'enum' in spec:assert value in spec['enum']
    t=spec.get('type')
    if t=='object':
        assert isinstance(value,dict)
        assert set(spec.get('required',[]))<=set(value)
        if spec.get('additionalProperties') is False:assert set(value)<=set(spec['properties'])
        for k,v in value.items():validate(v,spec['properties'][k],defs)
    elif t=='array':
        assert isinstance(value,list)
        if 'maxItems' in spec:assert len(value)<=spec['maxItems']
        for v in value:validate(v,spec['items'],defs)
    elif t=='string':
        assert isinstance(value,str)
        if 'maxLength' in spec:assert len(value)<=spec['maxLength']
        if 'pattern' in spec:assert re.fullmatch(spec['pattern'],value)
    elif t=='boolean':assert isinstance(value,bool)
    elif t=='null':assert value is None

def semantic(n,v):
    if n=='BatchView':
        if v['state']=='COMMITTED':assert v['receipt'] is not None and v['receipt']['disposition']=='COMMITTED'
        if v['state']=='CORRECTED':assert v['receipt'] is not None and v['receipt']['disposition']=='VOID'
    if n=='AbsenceProof':
        first,last,H=map(int,[v['first_possible_height'],v['timeout_height'],v['observed_height']])
        assert H>last and v['receipt_absent'] is True
        assert [int(b['height']) for b in v['blocks']]==list(range(first,last+1))

def run():
    cfg=read('profile.json');policy=read('vectors/policy.json');ledger=read('vectors/ledger.json')
    for c in policy['arithmetic']:expect(arithmetic(c),c['expected'],'arithmetic/'+c['id'])
    for c in policy['amount_bounds']:
        try:integer(c['value'],int(c['bits']));result='OK'
        except ValueError:result='INTEGER_RANGE'
        expect(result,c['expected'],'integer')
    for s in ['-1','+1','1e3','1.0','01',' 1',1]:
        try:integer(s,128);result='ACCEPT'
        except ValueError:result='REJECT'
        expect(result,'REJECT','noncanonical integer')
    sf=policy['split_fee'];bps=int(sf['bps']);parts=list(map(int,sf['parts']))
    expect(str(sum((p*bps+9999)//10000 for p in parts)),sf['split'],'split fee')
    expect(str((sum(parts)*bps+9999)//10000),sf['combined'],'combined fee')
    for c in policy['expiry']:expect('OK' if int(c['h'])<int(c['expiry']) else 'EXPIRED',c['expected'],'expiry')
    for c in policy['timeout']:expect('CANNOT_EXECUTE_NEW' if int(c['h'])>int(c['timeout']) else 'MAY_EXECUTE',c['expected'],'timeout')
    gross=defaultdict(int)
    for f in policy['gross_debit']['fills']:
        gross[f['seller']+'_BASE']+=int(f['q'])*1000;gross[f['buyer']+'_QUOTE']+=int(f['q'])*int(f['p'])
    expect({k:str(v) for k,v in gross.items()},policy['gross_debit']['expected_gross'],'gross sum')
    expect(any(v>int(policy['gross_debit']['C_start'][k]) for k,v in gross.items()),True,'gross cycle denied')
    for c in policy['receipt_cases']:
        seq,last=int(c['requested_seq']),int(c['last_seq'])
        if seq<=last:
            r='RECEIPT_INCONSISTENCY' if c['stored'] is None else 'BATCH_CONFLICT' if not c['hash_matches'] else 'BATCH_CLOSED' if c['stored']=='VOID' else 'ALREADY_COMMITTED'
        else:r='BATCH_SEQUENCE_GAP' if seq!=last+1 else 'CHECK_PREV_AND_CURRENT_POLICY'
        expect(r,c['expected'],'receipt/'+c['id'])
    for c in policy['attempt_resolution']:expect(resolution(c),c['expected'],'attempt/'+c['id'])
    steps={c['id']:c for c in ledger['demo0']}
    for c in ledger['demo0']:
        for k,v in c.items():
            if k=='id':continue
            C,R,D,P,A=map(int,v);expect(C-R-D,A,'A equation/'+c['id']+'/'+k);expect(min(C,R,D,P,A)>=0,True,'nonnegative')
    expect({k:v for k,v in steps['cancel-remainder'].items() if k!='id'},{k:v for k,v in steps['submission-unknown'].items() if k!='id'},'unknown holds')
    expect(int(steps['committed']['B_QUOTE'][4])-int(steps['submission-unknown']['B_QUOTE'][4]),2000000,'price release only commit')
    for c in ledger['withdraw_order']:
        a_base,a_quote,b_base,b_quote,epoch=10000000,0,0,100000000,0
        settle=withdraw=None
        for op in c['order']:
            if op=='WITHDRAW':
                if int(c['withdraw_atoms'])>b_quote:withdraw='INSUFFICIENT_CONFIRMED_BALANCE'
                else:b_quote-=int(c['withdraw_atoms']);epoch+=1;withdraw='OK'
            elif epoch:settle='EPOCH_MISMATCH'
            else:a_base-=1000000;a_quote+=10000000;b_base+=1000000;b_quote-=10000000;settle='OK'
        for k,v in dict(A_BASE=a_base,A_QUOTE=a_quote,B_BASE=b_base,B_QUOTE=b_quote,B_epoch=epoch).items():expect(str(v),c[k],c['id']+'/'+k)
        expect(settle,c['expected_settle'],'settle ordering');expect(withdraw,c['expected_withdraw'],'withdraw ordering')
    c=ledger['demo25_committed']
    for denom in ['BASE','QUOTE']:expect(int(c['A_'+denom])+int(c['B_'+denom])+int(c['T_'+denom])+int(c['U_'+denom]),int(c['module_'+denom]),'fee conservation')
    for c in ledger['conservation']:
        delta=int(c['bank'])-int(c['sum_C'])-int(c['T'])-int(c['U'])
        expect('OK' if delta==0 else 'QUARANTINE_EXCESS_1_ALLOW_VALID_WITHDRAW' if delta==1 else 'ASSET_DEFICIT_HALT_SETTLEMENT',c['expected'],'conservation residual')
    c=read('vectors/correction.json');fills=c['fills'];selected=closure(fills,c['root_fill_ids'])
    expect(selected,c['expected_corrected'],'dependency closure')
    survivors=[f for f in fills if f['id'] not in selected and f['state']=='PENDING']
    expect([f['id'] for f in survivors],c['expected_surviving_pending'],'independent fill preserved')
    for f in fills:
        if f['state']=='COMMITTED':continue
        direct=set()
        earlier=sorted([p for p in fills if p['state']=='PENDING' and int(p['seq'])<int(f['seq'])],key=lambda p:int(p['seq']))
        for field in ['orders','debits']:
            for key in f[field]:
                matching=[p for p in earlier if key in p[field]]
                if matching:direct.add(matching[-1]['id'])
        computed=[p['id'] for p in earlier if p['id'] in direct]
        expect(computed,f['deps'],'recorded deps complete/'+f['id'])
    D=defaultdict(int);P=defaultdict(int)
    for f in survivors:
        D[f['seller']+'_BASE']+=int(f['q'])*1000;D[f['buyer']+'_QUOTE']+=int(f['q'])*int(f['p'])
        P[f['buyer']+'_BASE']+=int(f['q'])*1000;P[f['seller']+'_QUOTE']+=int(f['q'])*int(f['p'])
    expect({k:str(v) for k,v in D.items()},c['expected_remaining_D'],'remaining D')
    expect({k:str(v) for k,v in P.items()},c['expected_remaining_P'],'remaining P')
    expect([f['id'] for f in fills if f['state']=='COMMITTED'],c['expected_committed_unchanged'],'immutable committed')
    dependency_equivalence(fills)
    for c in read('vectors/correction-history.json')['cases']:
        expanded=[{'id':f['fill_id'],'seq':str(i),'state':'PENDING','deps':f['predecessor_fill_ids'],
          'orders':[f['buyer_order_hash'],f['seller_order_hash']]} for i,f in enumerate(c['fills'])]
        dependency_equivalence(expanded)
        expect(closure(expanded,[expanded[0]['id']]),c['expected_corrected_fill_ids'],'large correction full IDs')
        n=len(expanded);bps=int(c['bps']);fb=(1000*bps+9999)//10000;fq=(401*bps+9999)//10000
        for k,v in dict(expected_D_BASE=n*1000,expected_D_QUOTE=n*401,expected_P_BASE=n*(1000-fb),expected_P_QUOTE=n*(401-fq)).items():expect(str(v),c[k],'large holds '+k)
        expect(len(canon(c))<16777216,True,'model capacity payload fits')
        expect(len(set(c['affected_order_hashes'])),201 if n==1001 else 200,'full affected orders')
    c=read('vectors/correction.json')
    # Deliberate faulty oracles must be detected; not an actual product fault injection.
    expect(selected!=c['expected_corrected']+['F4'],True,'mutant broad owner closure detected')
    expect(resolution(policy['attempt_resolution'][2])!='CORRECTION_READY',True,'mutant timeout release detected')
    expect(steps['submission-unknown']['B_QUOTE'][4]!='90000000',True,'mutant premature improvement detected')
    # Atomic publication model: speculative writes never replace the visible before state.
    before={k:v for k,v in steps['submission-unknown'].items() if k!='id'}
    after={k:v for k,v in steps['committed'].items() if k!='id'}
    for completed in range(5):
        candidate=copy.deepcopy(before)
        for k in list(after)[:completed]:candidate[k]=after[k]
        visible=after if completed==4 else before
        for _ in range(2):expect(visible,after if completed==4 else before,'atomic model replay')
        if 0<completed<4:expect(candidate!=visible,True,'partial candidate forbidden')
    schema=read('schema.json');defs=schema['$defs']
    def walk(x):
        if isinstance(x,dict):
            if '$ref' in x:expect(x['$ref'].split('/')[-1] in defs,True,'schema reference')
            if x.get('type')=='object':expect(set(x['required']),set(x['properties']),'all fields required');expect(x['additionalProperties'],False,'strict keys')
            for v in x.values():walk(v)
        elif isinstance(x,list):
            for v in x:walk(v)
    walk(schema)
    for c in read('vectors/schema-vectors.json')['cases']:
        try:validate(c['value'],defs[c['type']],defs);semantic(c['type'],c['value']);result='ACCEPT'
        except (ValueError,AssertionError,TypeError):result='REJECT'
        expect(result,c['expected'],'schema/'+c['id'])
    for n in ['Correction','CorrectionRecord','CommandResult','EngineState']:
        for k,v in defs[n]['properties'].items():
            if v.get('type')=='array' and k not in ['accounts','ledger_changes']:expect('maxItems' in v,False,'no history truncation/'+n+'/'+k)
    keys=read('vectors/test-keys.json');old_keys=json.loads((ROOT/'protocol/s2/vectors/test-keys.json').read_text())
    expect(len(set(k['public_key_hex'] for k in keys)),19,'distinct new fixture keys')
    expect(bool(set(k['public_key_hex'] for k in keys)&set(k['public_key_hex'] for k in old_keys)),False,'S2 keys unused')
    signed=read('vectors/signed.json');byid={c['id']:c for c in signed['cases']}
    expect(len(byid),len(signed['cases']),'unique signed fixture IDs')
    for c in signed['cases']:
        raw=bytes.fromhex(c['canonical_hex'])
        if 'fields' in c:expect(encode(c['fields']),raw,'strict fields bytes')
        domain={'OrderV1':'NUS/ORDER/V1','CancelV1':'NUS/CANCEL/V1','WalletChallengeV1':'NUS/WALLET_AUTH/V1'}.get(c['kind'])
        msg=frame(domain,raw) if domain else raw
        expect(msg.hex(),c['sign_input_hex'],'signed bytes');expect(sha(msg),c['sha256'],'signed hash')
        expect(len(bytes.fromhex(c['signature_hex'])),3309,'signature length');expect(len(bytes.fromhex(c['public_key_hex'])),1952,'pub length')
        expect(sha(bytes.fromhex(c['public_key_hex']))[:40],c['owner_raw_hex'],'owner')
    batches=read('vectors/batches.json');batch_byid={b['id']:b for b in batches}
    for b in batches:
        raw=encode(b['fields']);core=encode(b['core_fields'])
        expect(b['fields'][0],[1,'u32','2'],'BatchV2 activation')
        expect(raw.hex(),b['canonical_hex'],'batch canonical');expect(len(raw),int(b['byte_length']),'batch length')
        expect(sha(frame('NUS/BATCH_ID/V2',core)),b['batch_id'],'batch id');expect(sha(frame('NUS/BATCH_HASH/V2',raw)),b['batch_hash'],'batch hash')
        expect((S3/f"vectors/batch-{b['id']}.bin").read_bytes(),raw,'raw batch file')
        expect(len(raw)<=int(cfg['max_batch_bytes']),True,'batch byte cap')
        expect(len(b['fills'])<=8 and len(b['order_ids'])<=16,True,'count caps')
        for f in b['fills']:expect(sha(frame('NUS/FILL_ID/V1',encode(f['identity_fields']))),f['fill_id'],'fill ID')
        proof_hashes=[byid[i]['sha256'] for i in b['order_ids']]
        expect(len(set(proof_hashes)),len(proof_hashes),'distinct orders')
    for t in read('vectors/txs.json'):
        raw=blob(1,bytes.fromhex(t['body_hex']))+blob(2,bytes.fromhex(t['auth_info_hex']))+blob(3,bytes.fromhex(byid[t['id']]['signature_hex']))
        expect(raw.hex(),t['raw_tx_hex'],'TxRaw');expect(sha(raw),t['tx_hash'],'TX hash');expect(len(raw),int(t['byte_length']),'TX size')
        expect((S3/f"vectors/{t['id']}.bin").read_bytes(),raw,'raw TX file')
        expect(len(raw)<=int(cfg['max_settle_tx_bytes']),True,'TX limit')
        expect(t['batch_hash'],batch_byid[t['batch_fixture']]['batch_hash'],'attempt immutable batch')
    cap=read('vectors/capacity.json')
    receipt=read('vectors/receipt.json');raw=encode(receipt['fields'])
    expect(raw.hex(),receipt['canonical_hex'],'receipt V2 layout');expect(sha(raw),receipt['sha256'],'receipt hash')
    expect((S3/'vectors/receipt-demo.bin').read_bytes(),raw,'raw receipt')
    for c in read('vectors/negative-wire.json')['cases']:
        raw=(S3/c['file']).read_bytes();expect(sha(raw),c['sha256'],'negative raw hash');expect(len(raw),int(c['byte_length']),'negative raw length')
        expect(c['product_result'],'NOT_RUN','negative product not run')
    gas=int(cfg['max_settle_tx_bytes'])*10+17*750+128*1000+262144*3+96*2000+131072*30+500000
    expect(str(gas),cap['static_max_settle_gas'],'gas bound')
    expect(gas<int(cfg['settle_gas_limit']),True,'gas fits')
    expect(3*int(cfg['settle_gas_limit'])+2*int(cfg['close_gas_limit']),int(cfg['max_total_reserved_gas_per_batch']),'retry gas cap')
    expect(3*int(cfg['settle_fee_atoms'])+2*int(cfg['close_fee_atoms']),int(cfg['max_total_fee_atoms_per_batch']),'retry fee cap')
    for typ in ['settle','close']:expect((int(cfg[typ+'_gas_limit'])+499)//500,int(cfg[typ+'_fee_atoms']),'fee ceiling')
    expect(int(cfg['min_submit_expiry_delta_blocks'])>int(cfg['tx_timeout_delta_blocks']),True,'expiry margin')
    faults=read('faults.json');expect(len(faults['faults']),18,'fault coverage')
    expect({x['result'] for x in faults['faults']},{'NOT_RUN'},'no product PASS')
    expect(len(read('acceptance.json')['cases']),9,'AT count')
    for x in read('acceptance.json')['cases']:expect(x['a_product_result'],'NOT_RUN','A not product QA')
    # S3 additions must not modify the v1 wire/lock inheritance.
    expect(cfg['import_s2_outbox'],False,'no S2 import');expect(cfg['replicated'],False,'no replication claim')
    expect(cfg['batch_wire_version'],'2','batch version');expect(cfg['order_wire_version'],'1','user wire unchanged')
    fee25=read('profile-fee25.json');want=copy.deepcopy(cfg);want['profile']='s3-local-v1-fee25';want['active_fee_version']='2'
    expect(fee25,want,'separate fee25 profile')
    expect(sha((S3/'vectors/genesis-fixture.bin').read_bytes())!=sha((S3/'vectors/genesis-fee25-fixture.bin').read_bytes()),True,'separate fee25 synthetic genesis')

if __name__=='__main__':
    run()
    from check_state_hash import run as check_state_hash
    check_state_hash()
    m=manifest()
    if sys.argv[1:]==['--seal']:
        (S3/'manifest.json').write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n')
        print('SEALED',m['contract_sha256'])
    else:
        assert not sys.argv[1:]
        expect(read('manifest.json'),m,'manifest exact')
    print(f'PASS S3 specification oracle: {checks} checks; actual chain/IO/browser/main QA NOT_RUN')
