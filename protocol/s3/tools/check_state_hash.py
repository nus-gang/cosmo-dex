"""Read-only hash/replay checks using independently encoded length frames.

This validates the contract fixture, not a product engine or disk recovery.
"""
import base64
import copy
import hashlib
import json
import struct
from pathlib import Path

S3 = Path(__file__).resolve().parents[1]


def canonical(value):
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(',', ':')).encode('ascii')


def hash_value(kind, value):
    domain = ('NUS/S3/' + kind + '/V1').encode('ascii')
    payload = canonical(value)
    return hashlib.sha256(struct.pack('>I', len(domain)) + domain +
                          struct.pack('>Q', len(payload)) + payload).hexdigest()


def verify_step(before, step, defs, validate, objects=None):
    from evidence import verify_graph, resolve, OBJECTS
    object_store=OBJECTS if objects is None else objects
    after, result = step['after_state'], step['result']
    validate(after, defs['EngineState'], defs)
    validate(result, defs['CommandResult'], defs)
    h0, h1 = hash_value('ENGINE_STATE', before), hash_value('ENGINE_STATE', after)
    assert step['before_state_hash'] == h0
    assert step['after_state_hash'] == result['after_state_hash'] == h1
    assert step['state_json'] == base64.b64encode(canonical(after)).decode()
    assert step['result_json'] == base64.b64encode(canonical(result)).decode()
    assert step['result_hash'] == hash_value('COMMAND_RESULT', result)
    journal=step['journal_record']
    validate(journal,defs['JournalRecord'],defs)
    for field in ['before_state_hash','after_state_hash','result_hash','state_json','result_json']:
        assert journal[field]==step[field]
    assert journal['evidence_refs']==verify_graph([after,result],object_store)
    assert journal['context']==after['context'] and journal['command_seq']==result['command_seq']
    assert journal['snapshot']==after['chain_snapshot']
    assert after['context'] == before['context']
    assert int(after['last_command_seq']) == int(before['last_command_seq']) + 1
    assert after['stream_seq'] == after['last_command_seq'] == result['command_seq']
    snapshot = copy.deepcopy(after['chain_snapshot'])
    snapshot_id = snapshot.pop('snapshot_id')
    assert snapshot_id == hash_value('CHAIN_SNAPSHOT', snapshot) == result['snapshot_id']
    assert result['observed_height'] == snapshot['height']
    for field in ['applied_batches','resolution_receipts']:
        assert after[field][:len(before[field])]==before[field]
    old = before['corrections']
    assert after['corrections'][:len(old)] == old
    records = after['corrections'][len(old):]
    assert records and len(records) == len(result['correction_results'])
    assert len({r['correction_id'] for r in after['corrections']}) == len(after['corrections'])
    assert after['corrections'] == sorted(after['corrections'],key=lambda r:(int(r['command_seq']),r['correction_id']))
    assert result['corrected_fill_ids'] == [f for r in records for f in r['corrected_fill_ids']]
    assert result['affected_order_hashes'] == [o for r in records for o in r['affected_order_hashes']]
    assert result['request_hash'] == hash_value('CORRECTION_COMMAND',dict(context=after['context'],
        command_seq=result['command_seq'],snapshot_id=snapshot_id,
        correction_ids=[r['correction_id'] for r in records]))
    for record, audit in zip(records, result['correction_results']):
        assert record['before_state_hash'] == h0
        assert record['command_seq'] == result['command_seq']
        assert record['context'] == after['context']
        assert record['revision'] == '1'
        assert record['chain_snapshot_id'] == snapshot_id
        assert record['chain_height'] == snapshot['height']
        assert audit == dict(record, after_state_hash=h1)
        assert record['correction_id'] == hash_value('CORRECTION',dict(context=record['context'],
            void_batch=record['void_batch'],snapshot_id=snapshot_id,root_fill_ids=sorted(record['root_fill_ids'])))
        assert record['root_fill_ids'] == sorted(set(record['root_fill_ids']))
        assert record['root_fill_ids'] == record['void_batch']['fill_ids']
        assert record['resolution_receipt']['batch'] == record['void_batch']
        receipt=record['resolution_receipt']
        assert receipt['disposition'] == 'VOID'
        proof=json.loads(resolve(receipt['resolution_evidence_ref'],object_store))
        validate(proof,defs['ResolutionEvidence'],defs)
        assert receipt['resolution_evidence_hash']==hash_value('RESOLUTION_EVIDENCE',proof)
        assert receipt['failed_tx_hash']==proof['failed_tx_hash']
        assert receipt['batch']==proof['batch'] and receipt['context']==proof['context']
        assert receipt in after['resolution_receipts']
        applied=[a for a in after['applied_batches'] if a['batch_id']==receipt['batch']['batch_id']]
        assert len(applied)==1 and applied[0]['receipt_hash']==hashlib.sha256(canonical(receipt)).hexdigest()
        for tx in [receipt['terminal_tx'],proof['settle_attempts'][0]['confirmed_tx']]:
            assert tx['tx_hash']==hashlib.sha256(resolve(tx['raw_tx_ref'],object_store)).hexdigest()

    # Fixed model has two disjoint fills, each fully matched, fee 0 and no open remainder.
    ids=set(result['corrected_fill_ids'])
    for previous,current in zip(before['fills'],after['fills']):
        if current['fill_id'] not in ids:
            assert current == previous
        else:
            assert previous['state']=='PENDING' and current['state']=='CORRECTED'
            assert int(current['revision'])==int(previous['revision'])+1
            for key in ['fill_id','quantity_lots','execution_price_ticks','buyer_order_hash',
                        'seller_order_hash','dependency','command_seq','match_index']:
                assert current[key] == previous[key]
    for a in after['accounts']:
        for row in a['ledger']:
            debit = pending = 0
            for f in after['fills']:
                if f['state'] != 'PENDING': continue
                d = f['dependency']
                if a['owner']==d['seller_owner']:
                    debit += int(f['sell_D']) if row['denom']=='DEVBASE' else 0
                    pending += int(f['seller_P']) if row['denom']=='DEVQUOTE' else 0
                if a['owner']==d['buyer_owner']:
                    debit += int(f['buy_D']) if row['denom']=='DEVQUOTE' else 0
                    pending += int(f['buyer_P']) if row['denom']=='DEVBASE' else 0
            assert int(row['D'])==debit and int(row['P'])==pending
            assert int(row['R'])==0 and int(row['C'])==1000000000000
            assert int(row['A'])==int(row['C'])-debit>=0
    for o in after['orders']:
        v=o['view']
        assert int(v['filled_qty_lots'])==int(v['pending_qty_lots'])+int(v['settled_qty_lots'])+int(v['corrected_qty_lots'])
        assert int(v['filled_qty_lots'])<=int(v['max_qty_lots'])
    return copy.deepcopy(after)


def run():
    from check import validate
    fixture=json.loads((S3/'vectors/correction-state-hash.json').read_text())
    defs=json.loads((S3/'schema.json').read_text())['$defs']
    state=fixture['initial_state']
    validate(state,defs['EngineState'],defs)
    assert defs['EngineState']['properties']['corrections']['items']=={'$ref':'#/$defs/CorrectionRecord'}
    assert set(defs['Correction']['properties'])-set(defs['CorrectionRecord']['properties'])=={'after_state_hash'}
    # The state graph must not reach the current result, audit or journal hash.
    visited=set()
    def visit(spec):
        if isinstance(spec,dict):
            assert 'after_state_hash' not in spec.get('properties',{})
            if '$ref' in spec:
                name=spec['$ref'].split('/')[-1]
                assert name not in ['Correction','CommandResult','JournalRecord','ResultIndex','CommandReceipt']
                if name not in visited:
                    visited.add(name);visit(defs[name])
            for value in spec.values(): visit(value)
        elif isinstance(spec,list):
            for value in spec: visit(value)
    visit(defs['EngineState'])
    assert fixture['initial_state_hash']==hash_value('ENGINE_STATE',state)
    assert len(fixture['steps'])==2 and state['corrections']==[]
    # Replay from the same checkpoint twice; results and all accumulated records must be identical.
    for replay in range(2):
        state=copy.deepcopy(fixture['initial_state'])
        for i,step in enumerate(fixture['steps']):
            state=verify_step(state,step,defs,validate)
            assert len(state['corrections'])==i+1
            assert hash_value('ENGINE_STATE',state)==step['replay_hashes'][replay]
    first,second=fixture['steps']
    assert second['before_state_hash']==first['after_state_hash']
    assert first['result']['correction_results'][0]['after_state_hash']!=second['after_state_hash']
    assert second['after_state']['corrections'][0]==first['after_state']['corrections'][0]
    # Every record member is covered by full-state hashing, including historical before hashes.
    sensitivity=0
    for field,value in state['corrections'][0].items():
        mutant=copy.deepcopy(state)
        mutant['corrections'][0][field]=None if value is not None else ''
        assert hash_value('ENGINE_STATE',mutant)!=second['after_state_hash']
        sensitivity+=1
    mutations=[]
    for value in ['00'*32,None,second['after_state_hash']]:
        mutant=copy.deepcopy(second);mutant['after_state']['corrections'][-1]['after_state_hash']=value
        mutations.append(('state contains audit after hash',mutant))
    for target in ['before_state_hash','after_state_hash','correction_id']:
        mutant=copy.deepcopy(second);mutant['result']['correction_results'][0][target]='ff'*32
        # Even coherently rehashing the result cannot bypass its link to the state record.
        mutant['result_json']=base64.b64encode(canonical(mutant['result'])).decode()
        mutant['result_hash']=hash_value('COMMAND_RESULT',mutant['result'])
        mutant['journal_record']['result_json']=mutant['result_json']
        mutant['journal_record']['result_hash']=mutant['result_hash']
        mutations.append(('audit/state '+target,mutant))
    mutant=copy.deepcopy(second);mutant['after_state']['corrections'][0]['before_state_hash']='ff'*32
    mutations.append(('historical record tamper',mutant))
    mutant=copy.deepcopy(second);mutant['result']['correction_results']=[]
    mutations.append(('missing audit result',mutant))
    mutant=copy.deepcopy(second);mutant['after_state']['corrections'].reverse()
    mutations.append(('history reordering',mutant))
    mutant=copy.deepcopy(second);mutant['after_state']['corrections'].append(copy.deepcopy(mutant['after_state']['corrections'][-1]))
    mutations.append(('duplicate application',mutant))
    mutant=copy.deepcopy(second);mutant['after_state']['context']['service_schema']='s3/1'
    mutations.append(('old service schema',mutant))
    mutant=copy.deepcopy(second);mutant['after_state']['context']['service_schema']='s3/2'
    mutations.append(('old rc2 service schema',mutant))
    mutant=copy.deepcopy(second);mutant['journal_record']['evidence_refs']=[]
    mutations.append(('missing evidence closure',mutant))
    mutant=copy.deepcopy(second);mutant['result_json']=base64.b64encode(json.dumps(mutant['result']).encode()).decode()
    mutations.append(('noncanonical result bytes',mutant))
    mutant=copy.deepcopy(second);mutant['result_hash']='00'*32
    mutations.append(('wrong result hash',mutant))
    mutant=copy.deepcopy(second);mutant['journal_record']['after_state_hash']=first['after_state_hash']
    mutations.append(('journal state link',mutant))
    mutant=copy.deepcopy(second);mutant['result'].pop('correction_results')
    mutations.append(('old result shape',mutant))
    for label,mutant in mutations:
        try: verify_step(first['after_state'],mutant,defs,validate)
        except (AssertionError,ValueError,TypeError): pass
        else: raise AssertionError('accepted mutation: '+label)
    print(f'PASS S3 correction hash: 2 cumulative states, 4 replay steps, {sensitivity} field sensitivities, '
          f'{len(mutations)} rejected mutations; actual chain/WAL IO NOT_RUN')


if __name__=='__main__':
    run()
