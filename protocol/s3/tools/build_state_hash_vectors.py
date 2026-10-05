from evidence import put, TX, JSON, verify_graph
"""Author-only synthetic full-state hash fixtures; no runtime or WAL IO claim."""
import copy
from codec import S3, read, write, canon, sha, frame, b64, encode


def digest(domain, value):
    return sha(frame('NUS/S3/' + domain + '/V1', canon(value)))


def main():
    ctx = dict(service_schema='s3/3', chain_id='nus-s3-dev-1',
               genesis_hash=sha((S3/'vectors/genesis-fixture.bin').read_bytes()),
               contract_hash='11'*32, config_hash=sha((S3/'profile.json').read_bytes()),
               market_id='DEVBASE/DEVQUOTE', market_config_version='1')
    signed = {v['id']: v for v in read('vectors/signed.json')['cases']}
    inputs = [signed['capacity-valid-' + str(i)] for i in range(4)]
    owners = [b64(bytes.fromhex(o['owner_raw_hex'])) for o in inputs]
    snapshots = []
    for height in [200, 201, 202]:
        snap = dict(context=ctx, height=str(height), block_hash=sha(str(height).encode()),
                    block_time_unix_ms=str(1791187200000 + height*1000), accounts=[], assets=[],
                    operator=b64(bytes.fromhex(read('vectors/test-keys.json')[16]['owner_raw_hex'])),
                    operator_epoch='1', last_batch_seq=str(height-200),
                    last_batch_hash='00'*32 if height == 200 else sha(('batch'+str(height-200)).encode()),
                    terminal_batch_seqs=[str(i) for i in range(1, height-199)], owner_events=[])
        for i, o in enumerate(inputs):
            snap['accounts'].append(dict(owner=owners[i], key_type='ML-DSA-65',
                public_key=b64(bytes.fromhex(o['public_key_hex'])), epoch='0',
                account_number=str(i), sequence='0', gas_atoms='1000000000',
                assets=[dict(denom=d, bank_atoms='0', confirmed_atoms='1000000000000')
                        for d in ['DEVBASE', 'DEVQUOTE']]))
        snap['accounts'].sort(key=lambda a: bytes.fromhex(inputs[owners.index(a['owner'])]['owner_raw_hex']))
        snap['assets'] = [dict(denom=d, module_bank_atoms='4000000000000',
            sum_confirmed_atoms='4000000000000', treasury_atoms='0', unassigned_atoms='0',
            supply_atoms='4000000000000') for d in ['DEVBASE', 'DEVQUOTE']]
        snap['snapshot_id'] = digest('CHAIN_SNAPSHOT', snap)
        snapshots.append(snap)
    state = dict(context=ctx, last_command_seq='4', chain_snapshot=snapshots[0],
                 mode='CATCHING_UP', accounts=[], orders=[], fills=[], bindings=[], batches=[],
                 attempt_refs=[], dependencies=[], resolution_receipts=[], applied_batches=[],
                 corrections=[], latest_observation_ref=None, stream_seq='4')
    for i, o in enumerate(inputs):
        v = dict(order_id=o['fields'][8][2], order_hash=o['sha256'], owner_epoch='0',
                 admission_seq=str(i+1), side='SELL' if i%2 == 0 else 'BUY',
                 order_type='LIMIT_GTC', limit_price_ticks='1000000', max_qty_lots='1000000',
                 remaining_qty_lots='0', filled_qty_lots='1000000', corrected_qty_lots='0',
                 cancelled_qty_lots='0', state='FILLED_PENDING', revision='2',
                 pending_qty_lots='1000000', settled_qty_lots='0')
        state['orders'].append(dict(owner=owners[i], view=v,
            order_wire=b64(bytes.fromhex(o['canonical_hex'])), signature=b64(bytes.fromhex(o['signature_hex']))))
        state['bindings'].append(dict(owner=owners[i], owner_epoch='0', kind='ORDER',
            id=v['order_id'], request_hash=v['order_hash'], first_command_seq=str(i+1)))
    state['bindings'].sort(key=lambda a: bytes.fromhex(inputs[owners.index(a['owner'])]['owner_raw_hex']))
    identities = []
    for j in range(2):
        sell, buy = inputs[2*j:2*j+2]
        seq = str(2+2*j)
        fid = sha(frame('NUS/FILL_ID/V1', encode([[1,'utf8',ctx['chain_id']],
            [2,'utf8',ctx['market_id']], [3,'u64','1'], [4,'u64',seq], [5,'u32','0']])))
        dep = dict(fill_id=fid, predecessor_fill_ids=[], order_hashes=[sell['sha256'],buy['sha256']],
                   buyer_owner=owners[2*j+1], buyer_epoch='0', seller_owner=owners[2*j],
                   seller_epoch='0', source_snapshot_id=snapshots[0]['snapshot_id'])
        identity = dict(operator_epoch='1', batch_seq=str(j+1), batch_id=sha(('id'+str(j+1)).encode()),
                        batch_hash=sha(('batch'+str(j+1)).encode()),
                        previous_batch_hash='00'*32 if j==0 else identities[0]['batch_hash'], fill_ids=[fid])
        identities.append(identity)
        state['dependencies'].append(dep)
        state['fills'].append(dict(fill_id=fid, maker_order_hash=sell['sha256'], taker_order_hash=buy['sha256'],
            buyer_order_hash=buy['sha256'], seller_order_hash=sell['sha256'], command_seq=seq,
            match_index='0', quantity_lots='1000000', execution_price_ticks='1000000', fee_policy_version='1',
            fee_base_atoms='0', fee_quote_atoms='0', buy_D='1000000000000', sell_D='1000000000',
            buyer_P='1000000000', seller_P='1000000000000', snapshot_id=snapshots[0]['snapshot_id'],
            state='PENDING', revision='1', reason='', export_state='QUEUED_S3', submission_enabled=True,
            origin_operator_epoch='1', dependency=dep, batch=None))

    def balances(s):
        s['accounts'] = []
        for owner in [a['owner'] for a in snapshots[0]['accounts']]:
            ledger = []
            for denom in ['DEVBASE','DEVQUOTE']:
                debit = pending = 0
                for f in s['fills']:
                    if f['state'] != 'PENDING':
                        continue
                    dep = f['dependency']
                    if owner == dep['seller_owner']:
                        debit += int(f['sell_D']) if denom == 'DEVBASE' else 0
                        pending += int(f['seller_P']) if denom == 'DEVQUOTE' else 0
                    if owner == dep['buyer_owner']:
                        debit += int(f['buy_D']) if denom == 'DEVQUOTE' else 0
                        pending += int(f['buyer_P']) if denom == 'DEVBASE' else 0
                ledger.append(dict(denom=denom, C='1000000000000', R='0', D=str(debit),
                                   P=str(pending), A=str(1000000000000-debit)))
            s['accounts'].append(dict(owner=owner, owner_epoch='0', ledger=ledger, withdraw_frozen=False))

    balances(state)
    fixture = dict(scope='SYNTHETIC_HASH_REPLAY_ONLY; schema-complete state/results; synthetic batch/proof identities; '
                        'contract_hash 11.. and journal previous_commit_hash 77.. are explicit synthetic identifiers; no SDK, failure-proof validation, WAL frame hash or WAL IO',
                   initial_state=copy.deepcopy(state), initial_state_hash=digest('ENGINE_STATE',state), steps=[])
    for i in range(2):
        before = copy.deepcopy(state)
        before_hash = digest('ENGINE_STATE', before)
        identity = identities[i]
        snapshot = snapshots[i+1]
        raw = canon(dict(scope='SYNTHETIC_CLOSE_TX', seq=str(i+1)))
        failed_raw=canon(dict(scope='SYNTHETIC_FAILED_SETTLE',seq=str(i+1)))
        failed_tx=dict(tx_hash=sha(failed_raw),raw_tx_ref=put(failed_raw,TX),height=before['chain_snapshot']['height'],
            tx_index='0',block_hash=before['chain_snapshot']['block_hash'],abci_code='1',codespace='synthetic',
            gas_wanted='10000000',gas_used='1',
            raw_block_response_ref=put(canon(dict(scope='SYNTHETIC_FAILURE_BLOCK',seq=str(i+1)))),
            raw_results_response_ref=put(canon(dict(scope='SYNTHETIC_FAILURE_RESULTS',code='1'))))
        attempt=dict(context=ctx,batch=identity,attempt_no='1',kind='SETTLE',state='INCLUDED_FAILURE',
            operator=before['chain_snapshot']['operator'],operator_epoch='1',account_number='0',account_sequence=str(i),
            timeout_height=str(208+i),first_possible_height=str(200+i),gas_limit='10000000',fee_atoms='20000',
            raw_tx_ref=failed_tx['raw_tx_ref'],tx_hash=failed_tx['tx_hash'],broadcast_count='1',
            confirmed_tx=failed_tx,absence_proof=None)
        proof=dict(context=ctx,batch=identity,observed_snapshot=before['chain_snapshot'],settle_attempts=[attempt],
            batch_lookup=dict(context=ctx,observed_height=before['chain_snapshot']['height'],
                snapshot_id=before['chain_snapshot']['snapshot_id'],requested_seq=identity['batch_seq'],
                last_seq=before['chain_snapshot']['last_batch_seq'],last_hash=before['chain_snapshot']['last_batch_hash'],
                status='NOT_FOUND_AT_HEIGHT',receipt=None),failed_tx_hash=failed_tx['tx_hash'],rejection_code='EXPIRED')
        receipt = dict(context=ctx, batch=identity, disposition='VOID', batch_receipt_v2=None,
            failed_tx_hash=proof['failed_tx_hash'], resolution_evidence_hash=digest('RESOLUTION_EVIDENCE',proof),
            resolution_evidence_ref=put(canon(proof),JSON),
            terminal_tx=dict(tx_hash=sha(raw), raw_tx_ref=put(raw, TX), height=snapshot['height'], tx_index='0',
                block_hash=snapshot['block_hash'], abci_code='0', codespace='', gas_wanted='3000000', gas_used='1',
                raw_block_response_ref=put(canon(dict(scope='SYNTHETIC_BLOCK',height=snapshot['height']))),
                raw_results_response_ref=put(canon(dict(scope='SYNTHETIC_RESULTS',code='0')))))
        seq = str(5+i)
        record = dict(context=ctx, void_batch=identity, resolution_receipt=receipt,
            chain_snapshot_id=snapshot['snapshot_id'], chain_height=snapshot['height'],
            root_fill_ids=identity['fill_ids'], corrected_fill_ids=identity['fill_ids'],
            affected_order_hashes=[o['sha256'] for o in inputs[2*i:2*i+2]], cancelled_order_hashes=[],
            surviving_fill_ids=[f['fill_id'] for j,f in enumerate(state['fills']) if j!=i and f['state']=='PENDING'],
            before_state_hash=before_hash, command_seq=seq, revision='1')
        record['correction_id'] = digest('CORRECTION',dict(context=ctx,void_batch=identity,
            snapshot_id=snapshot['snapshot_id'],root_fill_ids=sorted(record['root_fill_ids'])))
        state['corrections'].append(record)
        state['chain_snapshot'] = snapshot
        state['last_command_seq'] = state['stream_seq'] = seq
        state['fills'][i].update(state='CORRECTED',revision='2',reason='EXPIRED',export_state='TERMINAL_S3',batch=identity)
        for o in state['orders'][2*i:2*i+2]:
            o['view'].update(state='CORRECTED',revision='3',pending_qty_lots='0',corrected_qty_lots='1000000')
        state['resolution_receipts'].append(receipt)
        state['batches'].append(dict(context=ctx,batch=identity,state='CORRECTED',revision='2',
            observed_height=snapshot['height'],reason='EXPIRED',seal_purpose='RESOLVE_FAILURE',attempt_hashes=[],
            receipt=dict(context=ctx,batch=identity,disposition='VOID',terminal_height=snapshot['height'],
                         terminal_tx_hash=receipt['terminal_tx']['tx_hash'],batch_receipt_v2=None)))
        state['applied_batches'].append(dict(batch_id=identity['batch_id'],receipt_hash=sha(canon(receipt)),
            revision='1',command_seq=seq,snapshot_id=snapshot['snapshot_id']))
        balances(state)
        after_hash = digest('ENGINE_STATE', state)
        audit = dict(record, after_state_hash=after_hash)
        request_hash = digest('CORRECTION_COMMAND',dict(context=ctx,command_seq=seq,
            snapshot_id=snapshot['snapshot_id'],correction_ids=[record['correction_id']]))
        changes=[]
        for old,new in zip(before['accounts'],state['accounts']):
            for a,b in zip(old['ledger'],new['ledger']):
                if a!=b: changes.append(dict(owner=new['owner'],before=a,after=b))
        result = dict(command_seq=seq,kind='CORRECTION',request_hash=request_hash,code='OK',state='LOCAL_ACCEPTED',
            observed_height=snapshot['height'],snapshot_id=snapshot['snapshot_id'],
            affected_order_hashes=record['affected_order_hashes'],created_fill_ids=[],corrected_fill_ids=identity['fill_ids'],
            ledger_changes=changes,after_state_hash=after_hash,committed_fill_ids=[],
            applied_batch_ids=[identity['batch_id']],correction_results=[audit])
        result_hash=digest('COMMAND_RESULT',result)
        journal = dict(context=ctx, command_seq=seq, previous_commit_hash='77'*32,
            command_kind='CORRECTION', recorded_at_unix_ms=snapshot['block_time_unix_ms'],
            request_wire='', signature='', signature_hash=sha(b''), snapshot=snapshot,
            observation=dict(snapshot_id=snapshot['snapshot_id'],observed_height=snapshot['height'],
                cursor_height=snapshot['height'],received_at_unix_ms=snapshot['block_time_unix_ms'],
                block_age_ms='0',query_latency_ms='0',last_success_age_ms='0',catching_up=False,fresh=True),
            before_state_hash=before_hash,after_state_hash=after_hash,result_hash=result_hash,
            state_json=b64(canon(state)),result_json=b64(canon(result)),
            external_event_ids=[record['correction_id']],evidence_refs=verify_graph([state,result]))
        fixture['steps'].append(dict(id='correction-'+str(i+1), before_state_hash=before_hash,
            after_state=copy.deepcopy(state),after_state_hash=after_hash,result=result,result_hash=result_hash,
            state_json=b64(canon(state)),result_json=b64(canon(result)),
            journal_record=journal,replay_hashes=[after_hash,after_hash]))
    write('vectors/correction-state-hash.json',fixture)
    print('generated full-state correction hash fixtures: 2 corrections, 2 replays each')


if __name__ == '__main__':
    main()
