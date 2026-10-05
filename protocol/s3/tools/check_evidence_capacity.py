"""Read-only rc3 evidence/capacity contract tests. Synthetic, NOT engine/chain IO."""
import base64
import copy
import hashlib
import json
from codec import S3, canon, b64, read
from evidence import reference, resolve, refs, verify_graph, RPC, TX, JSON, OBJECTS
from capacity import certificate, Bounds, admit, payload_fit, LIMIT
from check_state_hash import hash_value, verify_step


def sized_json(size):
    raw=b'{"jsonrpc":"2.0","id":1,"result":{"height":"201","txs_results":[]}}'
    assert size>=len(raw)
    return raw+b' '*(size-len(raw))


def large_step(fixture, raw):
    step=copy.deepcopy(fixture['steps'][0])
    state,result,journal=step['after_state'],step['result'],step['journal_record']
    for receipt in [state['resolution_receipts'][0],state['corrections'][0]['resolution_receipt'],result['correction_results'][0]['resolution_receipt']]:
        receipt['terminal_tx']['raw_results_response_ref']=reference(raw)
    state['applied_batches'][0]['receipt_hash']=hashlib.sha256(canon(state['resolution_receipts'][0])).hexdigest()
    h=hash_value('ENGINE_STATE',state)
    result['after_state_hash']=result['correction_results'][0]['after_state_hash']=h
    step.update(after_state_hash=h,state_json=b64(canon(state)),result_json=b64(canon(result)),
                result_hash=hash_value('COMMAND_RESULT',result),replay_hashes=[h,h])
    for key in ['after_state_hash','state_json','result_json','result_hash']:journal[key]=step[key]
    # Object closure supplied separately; raw is not written by the read-only check.
    objects={p.name:p.read_bytes() for p in OBJECTS.iterdir() if p.is_file()}
    objects[reference(raw)['sha256']]=raw
    journal['evidence_refs']=verify_graph([state,result],objects)
    return step,objects


def history_state(fixture, count, orders, bps):
    """Fully shaped storage stress input, not a reachable/signature-valid trace.

    count-2 historical corrected fills, two pending; one retained full correction.
    The existing independent dependency/fee oracle separately exercises all-pending
    1000/1001 closure. No acceptance of unbounded future corrections is asserted.
    """
    state=copy.deepcopy(fixture['steps'][0]['after_state'])
    state['orders']=[];state['bindings']=[];state['fills']=[];state['dependencies']=[]
    templates=fixture['initial_state']
    hs=lambda label:hashlib.sha256(label.encode()).hexdigest()
    for i in range(orders):
        order=copy.deepcopy(templates['orders'][i%4]);view=order['view']
        view.update(order_id=hs('order-id-'+str(i)),order_hash=hs('order-'+str(i)),admission_seq=str(i+1))
        state['orders'].append(order)
        state['bindings'].append(dict(owner=order['owner'],owner_epoch='0',kind='ORDER',id=view['order_id'],
                                    request_hash=view['order_hash'],first_command_seq=str(i+1)))
    for i in range(count):
        fill=copy.deepcopy(templates['fills'][i%2]);fid=hs('history-fill-'+str(i))
        buy=state['orders'][i%orders]['view']['order_hash'];sell=state['orders'][(i+1)%orders]['view']['order_hash']
        dep=copy.deepcopy(fill['dependency']);dep.update(fill_id=fid,order_hashes=[buy,sell],
                predecessor_fill_ids=[] if i==0 else [state['fills'][-1]['fill_id']])
        fb=(1000*bps+9999)//10000;fq=(401*bps+9999)//10000
        fill.update(fill_id=fid,command_seq=str(i+2),buyer_order_hash=buy,seller_order_hash=sell,
                    maker_order_hash=sell,taker_order_hash=buy,dependency=dep,quantity_lots='1',
                    execution_price_ticks='401',fee_policy_version='1' if bps==0 else '2',
                    fee_base_atoms=str(fb),fee_quote_atoms=str(fq),buy_D='401',sell_D='1000',
                    buyer_P=str(1000-fb),seller_P=str(401-fq),state='CORRECTED' if i<count-2 else 'PENDING',
                    revision='2' if i<count-2 else '1')
        state['fills'].append(fill);state['dependencies'].append(dep)
    record=state['corrections'][0]
    record['corrected_fill_ids']=[f['fill_id'] for f in state['fills'][:-2]]
    record['affected_order_hashes']=[o['view']['order_hash'] for o in state['orders']]
    record['surviving_fill_ids']=[f['fill_id'] for f in state['fills'][-2:]]
    return state


def run():
    from check import validate
    defs=read('schema.json')['$defs'];fixture=read('vectors/correction-state-hash.json')
    profile=read('profile.json')
    for key,want in dict(max_raw_rpc_response_bytes=16777216,max_evidence_object_bytes=262144,
        max_raw_responses_per_batch=80,max_raw_txs_per_batch=5,max_attempt_objects_per_batch=25,
        max_canonical_objects_per_batch=96,max_drain_records_per_pending_fill=40,drain_fixed_records=2,
        max_snapshot_owner_events=128,max_snapshot_terminal_slots=128,storage_allocation_unit_bytes=4096,
        storage_metadata_reserve_per_object_bytes=8192,max_commit_marker_bytes=4096).items():
        assert profile[key]==str(want),(key,profile[key])
    for field in ['owner_events','terminal_batch_seqs']:
        assert defs['ChainSnapshot']['properties'][field]['maxItems']==128
    try:Bounds(1,1).spec({'type':'array','items':{'type':'boolean'}},'Unknown.history')
    except ValueError:pass
    else:raise AssertionError('missing capacity bound accepted')
    fixture_result=read('vectors/evidence-capacity.json')
    # Regression for the reported rc2 failure: preserve all 3 MiB original bytes.
    raw=sized_json(3145728);step,objects=large_step(fixture,raw)
    assert hashlib.sha256(raw).hexdigest()=='3a6bc3ad4851b52ca33f56f5fa632b23722ee6eaf066e4682cc7aac4978d31d2'
    for name,value in [('EngineState',step['after_state']),('CommandResult',step['result']),('JournalRecord',step['journal_record'])]:
        validate(value,defs[name],defs)
    assert resolve(reference(raw),objects)==raw
    assert json.loads(raw)==json.loads(raw.rstrip())
    assert reference(raw)!=reference(raw.rstrip())
    got={'raw_response_bytes':len(raw),'raw_response_sha256':reference(raw)['sha256'],
         'state_bytes':len(canon(step['after_state'])),'result_bytes':len(canon(step['result'])),
         'journal_payload_bytes':len(canon(step['journal_record'])),'after_state_hash':step['after_state_hash'],
         'result_hash':step['result_hash']}
    assert got==fixture_result['large_raw_regression']
    for _ in range(2):verify_step(fixture['initial_state'],step,defs,validate,objects)
    assert payload_fit(canon(step['journal_record']))
    # Same ref and same raw bytes in history, audit, result and original receipt.
    stored=step['after_state']['resolution_receipts'][0]
    assert stored==step['after_state']['corrections'][0]['resolution_receipt']==step['result']['correction_results'][0]['resolution_receipt']
    assert stored['terminal_tx']['raw_results_response_ref']==reference(raw)
    assert sum(r['sha256']==reference(raw)['sha256'] for r in step['journal_record']['evidence_refs'])==1
    # Mutations are rejected even with unchanged state/result hash values.
    rejected=0
    for mode in ['missing','truncated','one-byte-tamper','canonicalized']:
        damaged=dict(objects);key=reference(raw)['sha256']
        if mode=='missing':damaged.pop(key)
        elif mode=='truncated':damaged[key]=raw[:-1]
        elif mode=='one-byte-tamper':damaged[key]=b'X'+raw[1:]
        else:damaged[key]=canon(json.loads(raw))
        try:verify_graph([step['after_state'],step['result']],damaged)
        except ValueError:rejected+=1
        else:raise AssertionError(mode)
    for field,value in [('byte_length','03145728'),('byte_length','3145727'),('sha256','../outside'),('media_type',TX)]:
        ref=reference(raw);ref[field]=value
        try:resolve(ref,objects)
        except ValueError:rejected+=1
        else:raise AssertionError(field)
    for bad in [b'{"a":1,"a":2}',b'{"a":NaN}',b'{bad','{"a":1}'.encode('utf-16')]:
        ref=reference(bad)
        try:resolve(ref,{ref['sha256']:bad})
        except ValueError:rejected+=1
        else:raise AssertionError('invalid raw JSON')
    old=copy.deepcopy(stored);old['terminal_tx']['raw_results_response']=b64(raw)
    try:validate(old,defs['ResolutionReceipt'],defs)
    except (AssertionError,ValueError):rejected+=1
    else:raise AssertionError('rc2 inline field')
    wrong=copy.deepcopy(stored);wrong['terminal_tx']['raw_results_response_ref']['media_type']=TX
    try:validate(wrong,defs['ResolutionReceipt'],defs)
    except (AssertionError,ValueError):rejected+=1
    else:raise AssertionError('role mismatch')
    max_raw=sized_json(LIMIT);max_ref=reference(max_raw)
    assert resolve(max_ref,{max_ref['sha256']:max_raw})==max_raw
    try:reference(max_raw+b' ')
    except ValueError:rejected+=1
    else:raise AssertionError('RPC limit plus one')
    # Framing boundary is measured before JSON/state decoding. These two raw
    # payloads are valid canonical JournalRecord shapes, not valid decoded states.
    for size,want in [(LIMIT,True),(LIMIT+1,False)]:
        j=copy.deepcopy(step['journal_record']);j['state_json']='';j['recorded_at_unix_ms']='1'
        gap=size-len(canon(j));extra=gap%4
        j['recorded_at_unix_ms']='1'*(1+extra);j['state_json']='A'*(gap-extra)
        validate(j,defs['JournalRecord'],defs)
        assert len(canon(j))==size and payload_fit(canon(j))==want
    reports=[]
    for count,orders,bps in [(1000,200,0),(1001,201,0),(1000,200,25),(1001,201,25)]:
        state=history_state(fixture,count,orders,bps)
        validate(state,defs['EngineState'],defs)
        cert=certificate(state)
        assert cert['admissible'] and len(state['fills'])==count and len(state['orders'])==orders
        assert len(state['corrections'][0]['corrected_fill_ids'])==count-2
        assert len(state['corrections'][0]['affected_order_hashes'])==orders
        assert cert['max_state_bytes']>=len(canon(state))
        assert admit(cert,cert['reserved_bytes']) and not admit(cert,cert['reserved_bytes']-1)
        # General free space can be zero after ACK. A correction spends its
        # dedicated reservation and does not rerun admission against free space.
        reserved=cert['reserved_bytes'];free=0
        cost=72+len(canon(step['journal_record']))+len(raw)
        assert not admit(cert,free) and cost<=reserved
        reserved-=cost
        assert reserved>=0 and free==0
        reports.append(dict(fills=count,orders=orders,bps=bps,certificate=cert))
        # An all-pending history may require N different future corrections,
        # each retaining survivor IDs. Never silently substitute the one-closure
        # happy path as the pre-ACK worst case.
        for fill in state['fills']:fill['state']='PENDING'
        worst=certificate(state)
        assert not worst['admissible']
    assert reports==fixture_result['cumulative_histories']
    # Evidence metadata bounds fit their explicit per-object envelope.
    bounds=Bounds(1001,201)
    for name in ['Attempt','ResolutionEvidence','ChainSnapshot']:
        assert bounds.size(name)<=262144,(name,bounds.size(name))
    assert rejected==fixture_result['rejected_mutations']
    print(f'PASS rc3 evidence/capacity: 3MiB raw -> {got["journal_payload_bytes"]}B WAL; '
          f'{rejected} negatives; RPC and WAL 16MiB/+1; four cumulative histories; reservation boundaries; '
          'all-pending unsafe admission rejected; filesystem/chain IO NOT_RUN')


if __name__=='__main__':run()
