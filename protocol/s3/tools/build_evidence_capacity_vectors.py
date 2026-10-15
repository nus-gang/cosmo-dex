"""Author-only deterministic size fixtures. No runtime resource allocation."""
from codec import read, write, canon
from check_evidence_capacity import sized_json, large_step, history_state
from capacity import certificate
from evidence import reference
from check_unicode_capacity import measure as unicode_capacity

fixture=read('vectors/correction-state-hash.json')
raw=sized_json(3145728);step,objects=large_step(fixture,raw)
out=dict(scope='SCHEMA/SERIALIZATION/RESERVATION MODEL ONLY; historical shapes are synthetic, not reachable signed engine traces; actual fsync/chain proof NOT_RUN',
    reported_rc2=dict(head='d45be33029705859b07a9516fd2229a56dc66f46',journal_payload_bytes=16874108,limit=16777216),
    large_raw_regression=dict(raw_response_bytes=len(raw),raw_response_sha256=reference(raw)['sha256'],
        state_bytes=len(canon(step['after_state'])),result_bytes=len(canon(step['result'])),
        journal_payload_bytes=len(canon(step['journal_record'])),after_state_hash=step['after_state_hash'],result_hash=step['result_hash']),
    cumulative_histories=[],rejected_mutations=15,
    boundaries=dict(wal_16777216='FIT',wal_16777217='REJECT_BEFORE_DECODE',rpc_16777216='ACCEPT_BYTES_ONLY',rpc_16777217='EVIDENCE_SIZE_HOLD'),
    all_pending_history='1000/1001 conservative worst-case reservation exceeds WAL limit: reject new ACK; never truncate history',
    known_limit='1000/1001 cumulative history is distinct from 1000/1001 simultaneously pending fills; dependency closure oracle remains separately required')
for count,orders,bps in [(1000,200,0),(1001,201,0),(1000,200,25),(1001,201,25)]:
    out['cumulative_histories'].append(dict(fills=count,orders=orders,bps=bps,certificate=certificate(history_state(fixture,count,orders,bps))))
out['unicode_capacity']=unicode_capacity()
write('vectors/evidence-capacity.json',out)
print('generated rc3 evidence/capacity fixtures')
