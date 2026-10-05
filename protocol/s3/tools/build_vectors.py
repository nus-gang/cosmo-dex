"""Authoring tool for public S3 fixtures; consumers run check.py, never regenerate."""
import copy
import sys
from codec import S3, ROOT, sha, frame, encode, blob, uint, read, write, address, canon, b64

GENESIS = b'NUS S3 PUBLIC SYNTHETIC FIXTURE 2026-10-05; not runtime genesis\n'
GH = sha(GENESIS)
GENESIS25 = b'NUS S3 PUBLIC SYNTHETIC FIXTURE FEE25 2026-10-05; not runtime genesis\n'
GH25 = sha(GENESIS25)

def order(key, name, side, qty, limit, cap=0, wide=False):
    hi = str(2**64-1)
    fields = [[1,'u32','1'],[2,'utf8','nus-s3-dev-1'],[3,'hex',GH25 if cap==25 else GH],
      [4,'utf8','x/exchange'],[5,'utf8','DEVBASE/DEVQUOTE'],[6,'u64','1'],
      [7,'hex',key['owner_raw_hex']],[8,'hex',key['public_key_hex']],
      [9,'hex',sha(name.encode())],[10,'u64',hi if wide else '0'],
      [11,'u32',str(side)],[12,'u64',str(limit)],[13,'u64',str(qty)],
      [14,'u32',str(cap)],[15,'utf8','RECEIVE_ASSET_V1'],
      [16,'u64',hi if wide else '200'],[17,'u32','1']]
    raw = encode(fields); framed = frame('NUS/ORDER/V1',raw)
    return dict(id=name,kind='OrderV1',fields=fields,canonical_hex=raw.hex(),
      sign_input_hex=framed.hex(),sha256=sha(framed),signature_hex='',key_id=key['id'],**{k:v for k,v in key.items() if k!='id'})

def prepare():
    keys=read('vectors/test-keys.json'); cases=[]
    for cap in [0,25]:
        for n,(side,qty,limit) in enumerate([(2,2000,10000),(1,1000,12000)]):
            cases.append(order(keys[n],f'demo-{cap}-{n}',side,qty,limit,cap))
    for n in range(16):
        cases.append(order(keys[n],f'capacity-{n}',2 if n%2==0 else 1,1000000,1000000,2**32-1,True))
        cases.append(order(keys[n],f'capacity-valid-{n}',2 if n%2==0 else 1,1000000,1000000,0,False))
    # New Cancel/Wallet inputs retain V1 wire exactly and use only new S3 public keys.
    seller=cases[0];k=keys[0]
    fields=[[1,'u32','1'],[2,'utf8','nus-s3-dev-1'],[3,'hex',GH],[4,'utf8','DEVBASE/DEVQUOTE'],
      [5,'hex',k['owner_raw_hex']],[6,'u64','0'],[7,'hex',seller['fields'][8][2]],
      [8,'hex',seller['sha256']],[9,'hex',sha(b'S3 cancel')],[10,'u64','200'],[11,'utf8','x/exchange']]
    for kind,name,domain,f in [('CancelV1','cancel','NUS/CANCEL/V1',fields),
      ('WalletChallengeV1','wallet','NUS/WALLET_AUTH/V1',[[1,'u32','1'],[2,'utf8','nus-s3-dev-1'],
       [3,'utf8','http://127.0.0.1:5173'],[4,'utf8','exchange-api'],[5,'hex',k['owner_raw_hex']],
       [6,'hex',sha(b'S3 challenge')],[7,'u64','1791187320'],[8,'hex',GH],[9,'u64','1791187200']])]:
        raw=encode(f);msg=frame(domain,raw)
        cases.append(dict(id=name,kind=kind,fields=f,canonical_hex=raw.hex(),sign_input_hex=msg.hex(),sha256=sha(msg),signature_hex='',key_id=k['id'],**{a:v for a,v in k.items() if a!='id'}))
    write('vectors/signed.json',{'scope':'PUBLIC_TEST_ONLY; pure ML-DSA, empty context; never runtime keys','genesis_hash':GH,'cases':cases})
    (S3/'vectors/genesis-fixture.bin').write_bytes(GENESIS)
    (S3/'vectors/genesis-fee25-fixture.bin').write_bytes(GENESIS25)

def make_batch(name, orders, fee_version, wide=False, seq=1, previous='00'*32):
    epoch=str(2**64-1) if wide else '1';fills=[]
    for n in range(len(orders)//2):
        sell,buy=orders[2*n:2*n+2];cmd=str(2**64-8+n) if wide else str(2+n)
        index=str(2**32-1) if wide else '0'
        identity=[[1,'utf8','nus-s3-dev-1'],[2,'utf8','DEVBASE/DEVQUOTE'],[3,'u64',epoch],[4,'u64',cmd],[5,'u32',index]]
        fid=sha(frame('NUS/FILL_ID/V1',encode(identity)))
        fields=[[1,'hex',fid],[2,'hex',sell['sha256']],[3,'hex',buy['sha256']],
          [4,'hex',buy['sha256']],[5,'hex',sell['sha256']],
      [6,'u64','1000000' if 'capacity' in name else '10000'],[7,'u64','1000000' if 'capacity' in name else '1000'],
          [8,'u64',str(fee_version)],[9,'u64',cmd],[10,'u32',index]]
        fills.append({'identity_fields':identity,'fill_id':fid,'fields':fields,'canonical_hex':encode(fields).hex()})
    core=[[1,'u32','2'],[2,'utf8','nus-s3-dev-1'],[3,'utf8','DEVBASE/DEVQUOTE'],[4,'u64',epoch],
      [5,'u64',str(2**64-1) if wide else str(seq)],[6,'hex',previous]]
    for o in sorted(orders,key=lambda o:o['sha256']):
        raw=blob(1,bytes.fromhex(o['canonical_hex']))+blob(2,bytes.fromhex(o['signature_hex']))
        core.append([8,'hex',raw.hex()])
    core += [[9,'hex',f['canonical_hex']] for f in fills]
    core += [[10,'hex',GH25 if fee_version==2 else GH],[11,'utf8','x/exchange'],[12,'u64','1']]
    bid=sha(frame('NUS/BATCH_ID/V2',encode(core)));fields=core[:6]+[[7,'hex',bid]]+core[6:]
    raw=encode(fields)
    return {'id':name,'scope':'STRUCTURAL_MAX; counters at exhaustion, not an executable state' if wide else 'SYNTHETIC_BUSINESS_INPUT',
      'order_ids':[o['id'] for o in orders],'fills':fills,'core_fields':core,'fields':fields,
      'batch_id':bid,'batch_hash':sha(frame('NUS/BATCH_HASH/V2',raw)),'canonical_hex':raw.hex(),'byte_length':str(len(raw))}

def envelopes():
    signed=read('vectors/signed.json');cases=[c for c in signed['cases'] if c['kind']!='SignDoc'];signed['cases']=cases;orders={o['id']:o for o in cases}
    batches=[make_batch('demo-0',[orders['demo-0-0'],orders['demo-0-1']],1),
      make_batch('demo-25',[orders['demo-25-0'],orders['demo-25-1']],2),
      make_batch('capacity-valid-8-16',[orders[f'capacity-valid-{i}'] for i in range(16)],1),
      make_batch('capacity-8-16',[orders[f'capacity-{i}'] for i in range(16)],1,True)]
    write('vectors/batches.json',batches)
    key=read('vectors/test-keys.json')[16];operator=address(bytes.fromhex(key['owner_raw_hex']))
    txs=[]
    for b in batches:
        for attempt,seq in [(1,0),(2,1)]:
            msg=blob(1,operator)+blob(2,bytes.fromhex(b['canonical_hex']))
            body=blob(1,blob(1,'/nus.exchange.s3.v1.MsgSettleBatch')+blob(2,msg))+uint(3,108)
            pkany=blob(1,'/cosmos.crypto.mldsa65.PubKey')+blob(2,blob(1,bytes.fromhex(key['public_key_hex'])))
            signer=blob(1,pkany)+blob(2,blob(1,uint(1,1)))+uint(3,seq,sdk=True)
            fee=blob(1,blob(1,'DEVGAS')+blob(2,'20000'))+uint(2,10000000)
            auth=blob(1,signer)+blob(2,fee)
            doc=blob(1,body)+blob(2,auth)+blob(3,'nus-s3-dev-1')+uint(4,16,sdk=True)
            id=f"tx-{b['id']}-{attempt}"
            c=dict(id=id,kind='SignDoc',canonical_hex=doc.hex(),sign_input_hex=doc.hex(),sha256=sha(doc),signature_hex='',key_id=key['id'],**{k:v for k,v in key.items() if k!='id'})
            cases.append(c)
            txs.append({'id':id,'batch_fixture':b['id'],'batch_id':b['batch_id'],'batch_hash':b['batch_hash'],
              'account_number':'16','account_sequence':str(seq),'timeout_height':'108','gas_limit':'10000000',
              'fee_atoms':'20000','body_hex':body.hex(),'auth_info_hex':auth.hex(),'sign_doc_hex':doc.hex()})
    write('vectors/signed.json',signed);write('vectors/txs.json',txs)

def finish():
    cases={c['id']:c for c in read('vectors/signed.json')['cases']}
    txs=read('vectors/txs.json')
    for t in txs:
        sig=bytes.fromhex(cases[t['id']]['signature_hex'])
        raw=blob(1,bytes.fromhex(t['body_hex']))+blob(2,bytes.fromhex(t['auth_info_hex']))+blob(3,sig)
        t.update(raw_tx_hex=raw.hex(),tx_hash=sha(raw),byte_length=str(len(raw)))
        (S3/f"vectors/{t['id']}.bin").write_bytes(raw)
    write('vectors/txs.json',txs)
    for b in read('vectors/batches.json'):
        (S3/f"vectors/batch-{b['id']}.bin").write_bytes(bytes.fromhex(b['canonical_hex']))
    cfg=read('profile.json')
    gas=139264*10+17*750+128*1000+262144*3+96*2000+131072*30+500000
    write('vectors/capacity.json',{'scope':'static serialized bound and fixed gas budget, NOT runtime gas measurement',
      'batches':[{k:b[k] for k in ['id','batch_id','batch_hash','byte_length']} for b in read('vectors/batches.json')],
      'txs':[{k:t[k] for k in ['id','tx_hash','byte_length']} for t in txs],
      'static_max_settle_gas':str(gas),'settle_gas_headroom':str(10000000-gas),
      'bound_equation':'139264*10 + 17*750 + 128*1000 + 262144*3 + 96*2000 + 131072*30 + 500000',
      'bound_precondition':'B must trace actual KV calls including Has and keys; no unbounded iteration in settlement',
      'envelope_reserve_bytes':'8192','boundaries':[
       {'field':k,'at':cfg[k],'at_plus_one':str(int(cfg[k])+1),'expected':'RESOURCE_LIMIT'}
       for k in ['max_batch_bytes','max_settle_tx_bytes','max_fills_per_batch','max_orders_per_batch','max_mldsa_checks_per_block']]})
    batches=read('vectors/batches.json');demo=batches[0];tx=txs[0]
    receipt_fields=[[1,'u32','2'],[2,'utf8','nus-s3-dev-1'],[3,'hex',GH],[4,'utf8','DEVBASE/DEVQUOTE'],
      [5,'u64','1'],[6,'hex',demo['batch_id']],[7,'hex',demo['batch_hash']],[8,'u64','101'],[9,'hex',tx['tx_hash']]]
    receipt=encode(receipt_fields)
    (S3/'vectors/receipt-demo.bin').write_bytes(receipt)
    write('vectors/receipt.json',{'scope':'SYNTHETIC_EXPECTATION_NOT_CHAIN_PROOF','fields':receipt_fields,'canonical_hex':receipt.hex(),'sha256':sha(receipt)})
    context={'service_schema':'s3/2','chain_id':'nus-s3-dev-1','genesis_hash':GH,'contract_hash':'11'*32,
      'config_hash':sha((S3/'profile.json').read_bytes()),'market_id':'DEVBASE/DEVQUOTE','market_config_version':'1'}
    identity={'operator_epoch':'1','batch_seq':'1','batch_id':demo['batch_id'],'batch_hash':demo['batch_hash'],
      'previous_batch_hash':'00'*32,'fill_ids':[f['fill_id'] for f in demo['fills']]}
    pub={'context':context,'batch':identity,'disposition':'COMMITTED','terminal_height':'101','terminal_tx_hash':tx['tx_hash'],'batch_receipt_v2':b64(receipt)}
    view={'context':context,'batch':identity,'state':'COMMITTED','revision':'2','observed_height':'101','reason':'','seal_purpose':'NORMAL','attempt_hashes':[tx['tx_hash']],'receipt':pub}
    absence={'tx_hash':tx['tx_hash'],'first_possible_height':'101','timeout_height':'108','observed_height':'109',
      'account_sequence':'0','last_batch_seq':'0','last_batch_hash':'00'*32,'receipt_absent':True,
      'blocks':[{'height':str(h),'block_hash':sha(f'synthetic block {h}'.encode()),
        'raw_block_response':b64(canon({'scope':'SYNTHETIC','height':str(h),'txs':[]})),
        'raw_results_response':b64(canon({'scope':'SYNTHETIC','height':str(h),'txs_results':[]}))} for h in range(101,109)],
      'observation_snapshot_id':sha(b'synthetic snapshot 109')}
    negatives=[]
    for id,change in [('missing-context',lambda v:v.pop('context')),('unknown-state',lambda v:v.update(state='FINAL')),
      ('overflow-revision',lambda v:v.update(revision=str(2**64))),('old-context',lambda v:v['context'].update(service_schema='1')),
      ('extra-key',lambda v:v.update(accepted=True)),('missing-receipt',lambda v:v.update(receipt=None))]:
        v=copy.deepcopy(view);change(v);negatives.append({'id':id,'type':'BatchView','value':v,'expected':'REJECT'})
    gap=copy.deepcopy(absence);gap['blocks'].pop(2)
    negatives.append({'id':'absence-gap','type':'AbsenceProof','value':gap,'expected':'REJECT'})
    equal=copy.deepcopy(absence);equal['observed_height']='108'
    negatives.append({'id':'timeout-equality-not-absence','type':'AbsenceProof','value':equal,'expected':'REJECT'})
    write('vectors/schema-vectors.json',{'scope':'SYNTHETIC_SHAPES_NOT_RUNTIME_AUTHORITY; contract hash 11.. is placeholder',
      'cases':[{'id':'committed-view','type':'BatchView','value':view,'expected':'ACCEPT'},
      {'id':'bounded-absence','type':'AbsenceProof','value':absence,'expected':'ACCEPT'}]+negatives})
    def changed_batch(fields):
        core=[f for f in fields if f[0]!=7]
        bid=sha(frame('NUS/BATCH_ID/V2',encode(core)))
        fields=[f if f[0]!=7 else [7,'hex',bid] for f in fields]
        return encode(fields)
    negative_wires=[]
    mutations=[]
    nine=copy.deepcopy(demo['fields']);pos=next(i for i,f in enumerate(nine) if f[0]==9)
    nine[pos:pos]=[copy.deepcopy(nine[pos]) for _ in range(8)]
    mutations.append(('nine-fills',changed_batch(nine),'RESOURCE_LIMIT','new slot; resource check precedes duplicate fill'))
    seventeen=copy.deepcopy(demo['fields']);pos=next(i for i,f in enumerate(seventeen) if f[0]==8)
    seventeen[pos:pos]=[copy.deepcopy(seventeen[pos]) for _ in range(15)]
    mutations.append(('seventeen-proofs',changed_batch(seventeen),'RESOURCE_LIMIT','new slot; resource check precedes duplicate proof'))
    duplicate=copy.deepcopy(demo['fields']);pos=next(i for i,f in enumerate(duplicate) if f[0]==9)
    duplicate.insert(pos,copy.deepcopy(duplicate[pos]))
    mutations.append(('duplicate-fill',changed_batch(duplicate),'DUPLICATE_FILL','new slot'))
    empty=[f for f in copy.deepcopy(demo['fields']) if f[0]!=9]
    mutations.append(('empty',changed_batch(empty),'EMPTY_BATCH','new slot; empty before unused proof'))
    mutations.append(('unknown-tag',bytes.fromhex(demo['canonical_hex'])+uint(13,0),'NON_CANONICAL_WIRE','new slot'))
    v1=copy.deepcopy(demo['fields']);v1[0]=[1,'u32','1'];core=[f for f in v1 if f[0]!=7]
    old_id=sha(frame('NUS/BATCH_ID/V1',encode(core)))
    v1=[f if f[0]!=7 else [7,'hex',old_id] for f in v1]
    mutations.append(('v1-batch-disabled',encode(v1),'UNSUPPORTED_VERSION','S3 genesis height >= 1; valid v1 layout and ID'))
    mutations.append(('batch-byte-limit',bytes(131073),'RESOURCE_LIMIT','reject before decode'))
    for id,raw,error,setup in mutations:
        filename=f'vectors/negative-{id}.bin';(S3/filename).write_bytes(raw)
        negative_wires.append({'id':id,'file':filename,'sha256':sha(raw),'byte_length':str(len(raw)),
          'expected':error,'setup':setup,'product_result':'NOT_RUN'})
    write('vectors/negative-wire.json',{'scope':'raw strict-wire/resource/business rejection inputs; product execution NOT_RUN','cases':negative_wires})
    build_history()

def build_history():
    history=[]
    for n,order_count in [(1000,200),(1001,201)]:
        for bps in [0,25]:
            ids=[sha(f'S3 synthetic capacity fill {i}'.encode()) for i in range(n)]
            orders=[sha(f'S3 synthetic capacity order {i}'.encode()) for i in range(order_count)]
            fee_base=(1000*bps+9999)//10000;fee_quote=(401*bps+9999)//10000
            latest={};fills=[]
            for i,fid in enumerate(ids):
                buyer,seller=orders[1+i%(order_count-1)],orders[0]
                predecessors=sorted({latest[o] for o in [buyer,seller] if o in latest})
                fills.append({'fill_id':fid,'predecessor_fill_ids':[ids[j] for j in predecessors],
                  'buyer_order_hash':buyer,'seller_order_hash':seller})
                latest[buyer]=latest[seller]=i
            history.append({'id':f'fills{n}-orders{order_count}-fee{bps}',
              'scope':'SYNTHETIC_INTERNAL_HISTORY_CAPACITY; not a reachable matching trace or valid signature fixture',
              'bps':str(bps),'q_per_fill':'1','price_per_fill':'401','affected_order_hashes':orders,
              'fills':fills,
              'expected_corrected_fill_ids':ids,'expected_D_BASE':str(n*1000),'expected_D_QUOTE':str(n*401),
              'expected_P_BASE':str(n*(1000-fee_base)),'expected_P_QUOTE':str(n*(401-fee_quote)),
              'expected_after_D_P':'0','expected_no_truncation':True})
    write('vectors/correction-history.json',{'scope':'SPECIFICATION_ONLY','cases':history})

if __name__=='__main__':
    {'prepare':prepare,'envelopes':envelopes,'finish':finish,'history':build_history}[sys.argv[1]]()
    print('generated',sys.argv[1],'; synthetic fixtures only')
