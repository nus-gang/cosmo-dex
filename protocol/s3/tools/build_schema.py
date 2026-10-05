"""Authoring helper: materialize the S3 service schema and fixed test matrix."""
import copy
import json
from codec import write, read, ROOT

s={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'urn:nus:s3:3','$defs':{}}
d=s['$defs']
for name,bits in [('U32',32),('U64',64),('Atoms',128)]:
    d[name]={'type':'string','pattern':'^(0|[1-9][0-9]*)$','maxLength':len(str(2**bits-1)),
      'description':f'Additional mandatory semantic check: value <= {2**bits-1}; never float/Number.'}
d['Hash']={'type':'string','pattern':'^[0-9a-f]{64}$'}
d['Bytes']={'type':'string','pattern':'^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$','maxLength':22369624}
d['Bool']={'type':'boolean'}
d['Text']={'type':'string','maxLength':256,
    'description':'At most 256 Unicode code points, not UTF-16 code units or UTF-8 bytes. Canonical ASCII JSON uses at most 12 bytes per code point plus 2 quotes (3074 bytes). No normalization or truncation; see SCHEMA.md and STORAGE.md.'}
def ref(n):return {'$ref':'#/$defs/'+n}
def en(*items):return {'type':'string','enum':list(items)}
def arr(n,maxn=None):
    r={'type':'array','items':ref(n)}
    if maxn is not None:r['maxItems']=maxn
    return r
def nullable(n):return {'anyOf':[ref(n),{'type':'null'}]}
def obj(n,props):
    d[n]={'type':'object','properties':{k:ref(v) if isinstance(v,str) else v for k,v in props.items()},'required':list(props),'additionalProperties':False}
obj('Context',{'service_schema':en('s3/3'),'chain_id':en('nus-s3-dev-1'),'genesis_hash':'Hash','contract_hash':'Hash','config_hash':'Hash','market_id':en('DEVBASE/DEVQUOTE'),'market_config_version':'U64'})
obj('BatchIdentity',{'operator_epoch':'U64','batch_seq':'U64','batch_id':'Hash','batch_hash':'Hash','previous_batch_hash':'Hash','fill_ids':arr('Hash',8)})
obj('EvidenceRef',{'sha256':'Hash','byte_length':'U64','media_type':en('application/json','application/vnd.nus.txraw','application/vnd.nus.s3+json')})
for name,media in [('RpcEvidenceRef','application/json'),('TxEvidenceRef','application/vnd.nus.txraw'),('CanonicalEvidenceRef','application/vnd.nus.s3+json')]:
    d[name]=copy.deepcopy(d['EvidenceRef']);d[name]['properties']['media_type']={'const':media}
obj('ConfirmedTx',{'tx_hash':'Hash','raw_tx_ref':'TxEvidenceRef','height':'U64','tx_index':'U32','block_hash':'Hash','abci_code':'U32','codespace':'Text','gas_wanted':'U64','gas_used':'U64','raw_block_response_ref':'RpcEvidenceRef','raw_results_response_ref':'RpcEvidenceRef'})
obj('AbsenceBlock',{'height':'U64','block_hash':'Hash','raw_block_response_ref':'RpcEvidenceRef','raw_results_response_ref':'RpcEvidenceRef'})
obj('AbsenceProof',{'tx_hash':'Hash','first_possible_height':'U64','timeout_height':'U64','observed_height':'U64','account_sequence':'U64','last_batch_seq':'U64','last_batch_hash':'Hash','receipt_absent':'Bool','blocks':arr('AbsenceBlock',8),'observation_snapshot_id':'Hash'})
obj('Attempt',{'context':'Context','batch':'BatchIdentity','attempt_no':'U32','kind':en('SETTLE','CLOSE'),
  'state':en('PREPARED','SUBMISSION_UNKNOWN','INCLUDED_SUCCESS','INCLUDED_FAILURE','EXPIRED_ABSENT_PROVEN'),
  'operator':'Bytes','operator_epoch':'U64','account_number':'U64','account_sequence':'U64','timeout_height':'U64',
  'first_possible_height':'U64','gas_limit':'U64','fee_atoms':'Atoms','raw_tx_ref':'TxEvidenceRef','tx_hash':'Hash',
  'broadcast_count':'U32','confirmed_tx':nullable('ConfirmedTx'),'absence_proof':nullable('AbsenceProof')})
obj('ResolutionEvidence',{'context':'Context','batch':'BatchIdentity','observed_snapshot':'ChainSnapshot',
  'settle_attempts':arr('Attempt',3),'batch_lookup':'BatchLookup','failed_tx_hash':'Hash','rejection_code':'Text'})
obj('ResolutionReceipt',{'context':'Context','batch':'BatchIdentity','disposition':en('COMMITTED','VOID'),
  'terminal_tx':'ConfirmedTx','batch_receipt_v2':nullable('Bytes'),'failed_tx_hash':nullable('Hash'),
  'resolution_evidence_hash':nullable('Hash'),'resolution_evidence_ref':nullable('CanonicalEvidenceRef')})
obj('PublicReceipt',{'context':'Context','batch':'BatchIdentity','disposition':en('COMMITTED','VOID'),
  'terminal_height':'U64','terminal_tx_hash':'Hash','batch_receipt_v2':nullable('Bytes')})
obj('StoredResolutionReceipt',{'context':'Context','batch':'BatchIdentity','disposition':en('COMMITTED','VOID'),
  'terminal_height':'U64','terminal_tx_hash':'Hash','batch_receipt_v2':nullable('Bytes'),
  'failed_tx_hash':nullable('Hash'),'resolution_evidence_hash':nullable('Hash')})
obj('BatchLookup',{'context':'Context','observed_height':'U64','snapshot_id':'Hash','requested_seq':'U64',
  'last_seq':'U64','last_hash':'Hash','status':en('FOUND','NOT_FOUND_AT_HEIGHT','RECEIPT_INCONSISTENCY'),
  'receipt':nullable('StoredResolutionReceipt')})
obj('BatchView',{'context':'Context','batch':'BatchIdentity',
  'state':en('SEALED','SUBMISSION_UNKNOWN','COMMITTED','REJECTED_FINAL','CLOSING','CORRECTED','RECOVERY_REQUIRED'),
  'revision':'U64','observed_height':'U64','reason':'Text','seal_purpose':en('NORMAL','RESOLVE_FAILURE'),'attempt_hashes':arr('Hash',5),'receipt':nullable('PublicReceipt')})
obj('AssetAccount',{'denom':en('DEVBASE','DEVQUOTE'),'bank_atoms':'Atoms','confirmed_atoms':'Atoms'})
obj('Account',{'owner':'Bytes','key_type':en('ML-DSA-65'),'public_key':'Bytes','epoch':'U64','account_number':'U64','sequence':'U64','gas_atoms':'Atoms','assets':arr('AssetAccount',2)})
obj('AssetTotals',{'denom':en('DEVBASE','DEVQUOTE'),'module_bank_atoms':'Atoms','sum_confirmed_atoms':'Atoms','treasury_atoms':'Atoms','unassigned_atoms':'Atoms','supply_atoms':'Atoms'})
obj('OwnerEvent',{'kind':en('WITHDRAW','BUMP_EPOCH','REVOKE_ORDER'),'owner':'Bytes','before_epoch':'U64','after_epoch':'U64','order_hash':nullable('Hash'),'denom':en('DEVBASE','DEVQUOTE','NONE'),'amount_atoms':'Atoms','request_id':'Hash','tx_hash':'Hash','tx_index':'U32'})
obj('ChainSnapshot',{'context':'Context','height':'U64','block_hash':'Hash','block_time_unix_ms':'U64','snapshot_id':'Hash',
  'accounts':arr('Account',16),'assets':arr('AssetTotals',2),'operator':'Bytes','operator_epoch':'U64',
  'last_batch_seq':'U64','last_batch_hash':'Hash','terminal_batch_seqs':arr('U64',128),'owner_events':arr('OwnerEvent',128)})
obj('Hold',{'owner':'Bytes','denom':en('DEVBASE','DEVQUOTE'),'C':'Atoms','R':'Atoms','D':'Atoms','P':'Atoms','A':'Atoms'})
obj('Correction',{'context':'Context','correction_id':'Hash','void_batch':'BatchIdentity','resolution_receipt':'ResolutionReceipt',
  'chain_snapshot_id':'Hash','chain_height':'U64','root_fill_ids':arr('Hash'),'corrected_fill_ids':arr('Hash'),
  'affected_order_hashes':arr('Hash'),'cancelled_order_hashes':arr('Hash'),'surviving_fill_ids':arr('Hash'),
  'before_state_hash':'Hash','after_state_hash':'Hash','command_seq':'U64','revision':'U64'})
# State commits the immutable record; the complete audit result is WAL-only.
d['CorrectionRecord']=copy.deepcopy(d['Correction'])
d['CorrectionRecord']['properties'].pop('after_state_hash')
d['CorrectionRecord']['required'].remove('after_state_hash')
obj('SettlementApply',{'context':'Context','chain_snapshot':'ChainSnapshot','receipts':arr('ResolutionReceipt'),
  'before_state_hash':'Hash','after_state_hash':'Hash','command_seq':'U64','stream_seq':'U64',
  'applied_batch_ids':arr('Hash'),'corrected_fill_ids':arr('Hash'),'holds':arr('Hold',32)})
obj('Dependency',{'fill_id':'Hash','predecessor_fill_ids':arr('Hash',4),'order_hashes':arr('Hash',2),
  'buyer_owner':'Bytes','buyer_epoch':'U64','seller_owner':'Bytes','seller_epoch':'U64','source_snapshot_id':'Hash'})
obj('Status',{'context':'Context','mode':en('OPEN','CATCHING_UP','STALE','CORRECTING','WITHDRAW_FROZEN','RECOVERY_REQUIRED'),
  'fresh':'Bool','applied_height':'U64','latest_observed_height':'U64','snapshot_id':'Hash','stream_seq':'U64','content_hash':'Hash',
  'inflight':nullable('BatchView'),'admission_enabled':'Bool','reason':'Text'})
obj('Error',{'context':'Context','code':'Text','retryable':'Bool','state':en('REJECTED','SUBMISSION_UNKNOWN','RECOVERY_REQUIRED'),
  'height':nullable('U64'),'batch_id':nullable('Hash'),'tx_hash':nullable('Hash')})
obj('WithdrawalReadiness',{'context':'Context','owner':'Bytes','state':en('FROZEN','UNSETTLED_HOLD','READY','REQUIRES_EXPLICIT_ABORT'),
  'prepared_height':'U64','observed_height':'U64','snapshot_id':'Hash','owner_epoch':'U64','command_seq':'U64',
  'remaining_D':'Atoms','remaining_P':'Atoms','unresolved_attempts':'U32','fresh':'Bool'})
# Materialize inherited definitions; no unresolved external refs or implicit S2 defaults.
old=json.loads((ROOT/'protocol/s2/schema.json').read_text())['$defs']
for n,v in old.items():
    if n not in d:d[n]=copy.deepcopy(v)
def extend(n,props):
    for k,v in props.items():
        d[n]['properties'][k]=ref(v) if isinstance(v,str) else v
        if k not in d[n]['required']:d[n]['required'].append(k)
extend('Network',{'profile':en('s3-local-v1','s3-local-v1-fee25'),'settlement_enabled':{'const':True},'profile_hash':'Hash'})
extend('OrderView',{'pending_qty_lots':'U64','settled_qty_lots':'U64',
  'state':en('OPEN','PARTIALLY_FILLED','FILLED_PENDING','FILLED_COMMITTED','CANCELLED_OFFCHAIN','EXPIRED','STP_CANCELLED','POLICY_REJECTED_REMAINDER','CORRECTED','REVOKED_ONCHAIN')})
extend('FillView',{'state':en('PENDING','SUBMISSION_UNKNOWN','COMMITTED','CORRECTED'),'batch':nullable('BatchIdentity'),
  'tx_hash':nullable('Hash'),'committed_height':nullable('U64'),'correction_id':nullable('Hash')})
extend('OutboxFill',{'state':en('PENDING','SUBMISSION_UNKNOWN','COMMITTED','CORRECTED'),
  'export_state':en('QUEUED_S3','SEALED_S3','TERMINAL_S3'),'submission_enabled':{'const':True},
  'origin_operator_epoch':'U64','dependency':'Dependency','batch':nullable('BatchIdentity')})
obj('AppliedBatch',{'batch_id':'Hash','receipt_hash':'Hash','revision':'U64','command_seq':'U64','snapshot_id':'Hash'})
extend('EngineState',{'accounts':arr('EngineAccount',16),'batches':arr('BatchView'),'attempt_refs':arr('CanonicalEvidenceRef'),
  'dependencies':arr('Dependency'),'resolution_receipts':arr('ResolutionReceipt'),'applied_batches':arr('AppliedBatch'),
  'corrections':arr('CorrectionRecord'),'latest_observation_ref':nullable('CanonicalEvidenceRef'),'stream_seq':'U64'})
extend('CommandResult',{'ledger_changes':arr('LedgerChange',32),'committed_fill_ids':arr('Hash'),'applied_batch_ids':arr('Hash'),'correction_results':arr('Correction')})
extend('JournalRecord',{'command_kind':en('ORDER','CANCEL','SNAPSHOT','EXPIRY','WITHDRAW_PREPARE','WITHDRAW_ABORT','CORRECTION','SEAL_BATCH','ATTEMPT','RESOLVE_ATTEMPT','SETTLEMENT_APPLY','VOID_BATCH'),'evidence_refs':arr('EvidenceRef')})
extend('CommitMarker',{'schema_version':en('s3/3')})
# Encoding limits are storage/input bounds, not estimates of RPC response length.
for name,size in [('OwnerBytes',20),('PublicKeyBytes',1952),('SignatureBytes',3309),('RequestBytes',16384),('ReceiptBytes',4096)]:
    d[name]=copy.deepcopy(d['Bytes']);d[name]['maxLength']=4*((size+2)//3)
for name,spec in d.items():
    for field,prop in spec.get('properties',{}).items():
        narrow={'owner':'OwnerBytes','operator':'OwnerBytes','buyer_owner':'OwnerBytes','seller_owner':'OwnerBytes',
                'public_key':'PublicKeyBytes','signature':'SignatureBytes','order_wire':'RequestBytes',
                'request_wire':'RequestBytes','batch_receipt_v2':'ReceiptBytes'}.get(field)
        if narrow:
            if prop==ref('Bytes'):spec['properties'][field]=ref(narrow)
            elif prop==nullable('Bytes'):spec['properties'][field]=nullable(narrow)
s['oneOf']=[ref(n) for n in ['Attempt','ResolutionReceipt','BatchLookup','BatchView','ChainSnapshot','Correction','SettlementApply','Status','Error','WithdrawalReadiness','Network','SignedCommand','CommandReceipt','LedgerView','BookSnapshot','JournalRecord','CommitMarker']]
write('schema.json',s)
fee25=copy.deepcopy(read('profile.json'));fee25['profile']='s3-local-v1-fee25';fee25['active_fee_version']='2'
write('profile-fee25.json',fee25)

new_errors=[]
for code,http,retry,state in [
 ('BATCH_CLOSED','409',False,'REJECTED'),('BATCH_CONFLICT','409',False,'REJECTED'),
 ('BATCH_SEQUENCE_GAP','409',False,'REJECTED'),('PREVIOUS_BATCH_HASH_MISMATCH','409',False,'REJECTED'),
 ('OPERATOR_UNAUTHORIZED','403',False,'REJECTED'),('OPERATOR_EPOCH_MISMATCH','409',False,'REJECTED'),
 ('EMPTY_BATCH','400',False,'REJECTED'),('DUPLICATE_FILL','409',False,'REJECTED'),
 ('RECEIPT_INCONSISTENCY','503',False,'RECOVERY_REQUIRED'),('ATTEMPT_UNRESOLVED','202',True,'SUBMISSION_UNKNOWN'),
 ('RETRY_BUDGET_EXHAUSTED','503',False,'RECOVERY_REQUIRED'),('ASSET_DEFICIT','503',False,'RECOVERY_REQUIRED'),
 ('KV_BUDGET_EXCEEDED','422',False,'REJECTED'),('UNEXPECTED_FINAL_REJECTION','503',False,'RECOVERY_REQUIRED'),
 ('EXPIRY_MARGIN','422',False,'REJECTED'),('UNSETTLED_HOLD','409',True,'REJECTED'),
 ('EVIDENCE_SIZE','503',False,'RECOVERY_REQUIRED'),('EVIDENCE_MISSING','503',False,'RECOVERY_REQUIRED'),
 ('EVIDENCE_MISMATCH','503',False,'RECOVERY_REQUIRED'),('STORAGE_CAPACITY','507',True,'REJECTED'),
 ('S3_CONTEXT_REQUIRED','400',False,'REJECTED')]:
    new_errors.append(dict(code=code,http_status=http,retryable=retry,state=state))
module_codes=['RESOURCE_LIMIT','NON_CANONICAL_WIRE','INTEGER_RANGE','UNSUPPORTED_VERSION','CONTEXT_MISMATCH',
 'OPERATOR_UNAUTHORIZED','RECEIPT_INCONSISTENCY','BATCH_CONFLICT','BATCH_CLOSED','BATCH_SEQUENCE_GAP',
 'PREVIOUS_BATCH_HASH_MISMATCH','OPERATOR_EPOCH_MISMATCH','KEY_LENGTH','ADDRESS_MISMATCH',
 'ACCOUNT_KEY_UNREGISTERED','ACCOUNT_KEY_MISMATCH','INVALID_SIGNATURE','ID_CONFLICT','EPOCH_MISMATCH',
 'ORDER_REVOKED','EXPIRED','MARKET_LIMIT','BPS_RANGE','FEE_CAP','FEE_GE_RECEIVE','CUMULATIVE_QTY_EXCEEDED',
 'INSUFFICIENT_CONFIRMED_BALANCE','DUPLICATE_FILL','EMPTY_BATCH','ASSET_DEFICIT','KV_BUDGET_EXCEEDED',
 'SEQUENCE_OVERFLOW','INVALID_ENVELOPE','UNSUPPORTED_OPTION']
write('errors.json',{'inherit':['protocol/v1/CONTRACT.md','protocol/v1/DECISION-PORT.md','protocol/v1/adr/G-FIX-01.md','protocol/s2/errors.json'],
  'codespace':'exchange_s3','abci_codes':{name:str(1001+i) for i,name in enumerate(module_codes)},
  'new_or_overridden':new_errors,'numeric_abci_mapping':'Chain uses module codes in CONTRACT; SDK ante codes remain SDK codes, codespace distinguishes them'})

faults=[
 ('F01','before batch WAL append','NO_BATCH_NO_BROADCAST'),
 ('F02','after immutable batch fsync before marker','UNKNOWN_TAIL_PRESERVE_NO_SEND'),
 ('F03','after batch marker before attempt','REPLAY_SAME_BATCH'),
 ('F04','after attempt TX fsync before marker','UNKNOWN_TAIL_NO_NEW_ENVELOPE'),
 ('F05','after attempt marker before socket send','QUERY_THEN_SAME_TX_ONLY'),
 ('F06','after broadcast before RPC headers','SUBMISSION_UNKNOWN_HOLDS_UNCHANGED'),
 ('F07','after RPC headers before JSON complete','SUBMISSION_UNKNOWN_HOLDS_UNCHANGED'),
 ('F08','after chain commit before RPC response','QUERY_ORIGINAL_RECEIPT_ONE_ASSET_EFFECT'),
 ('F09','after receipt fsync before engine apply','REPLAY_RECEIPT_AND_MATCHING_SNAPSHOT'),
 ('F10','after candidate engine apply before WAL fsync','NO_PARTIAL_PUBLIC_VIEW'),
 ('F11','after apply WAL fsync before marker','PRESERVE_TAIL_RECONCILE_ONCE'),
 ('F12','after apply marker before UI response','REPLAY_ONE_REVISION_NO_DOUBLE_RELEASE'),
 ('F13','after close receipt before correction plan','NO_REPLACEMENT_SEQ_UNTIL_APPLY'),
 ('F14','after correction closure before candidate state','REPLAY_SAME_FULL_DEPENDENCY_SET'),
 ('F15','after correction WAL fsync before marker','NO_PARTIAL_CORRECTION'),
 ('F16','after correction marker before snapshot publish','REPLAY_SAME_NEW_BOOK_AND_HOLDS'),
 ('F17','after snapshot temp fsync before rename','OLD_VALID_SNAPSHOT_PLUS_WAL'),
 ('F18','second writer lock / WAL corruption / disk reservation unavailable','REFUSE_WRITER_OR_HALT_WITH_EVIDENCE')]
write('faults.json',{'repetitions_per_variant':'3','replays_per_crash':'2','faults':[dict(id=i,point=p,expected=e,result='NOT_RUN') for i,p,e in faults],
 'F18_variants':['second_writer','header_bitflip','payload_bitflip','missing_marker_committed_frame','ENOSPC_injection','exact_16MiB_payload','16MiB_plus_1_payload','WAL_and_marker_common_rollback_external_ACK_detection','RPC_16MiB','RPC_16MiB_plus1','missing_raw_object','tampered_raw_object','wrong_raw_role','missing_transitive_ref','reserve_minus1_before_ACK','dedicated_reserve_with_general_free_zero','preallocated_space_crash_replay'],
 'execution_requirement':'Each variant on fresh S3 home; no wall-clock races; barrier/tx_index proof. Injection mechanism and NOT_RUN boundaries recorded.'})
matrix=[
 ('S3-AT01','B/C/D/E/F/G/J',['demo0','demo25','received_asset_withdraw'],'T02,T03,T15(partial)'),
 ('S3-AT02','B/C/G/J',['signature_bitflip','wrong_key','wrong_key_type','unregistered','wrong_chain','wrong_genesis','wrong_domain','wrong_operator','old_operator_epoch','binding_conflict','cumulative_over','buy_limit','sell_limit','maker_price','overflow','h199','h200','h201','fee0','fee25','ceil','cap','fee_ge_receive','revoke','bump_epoch'],'T01,T02'),
 ('S3-AT03','B/G/J',['valid_then_bad_signature','bad_signature_then_valid','valid_then_bad_quantity','bad_quantity_then_valid','valid_then_insufficient','insufficient_then_valid','gross_cycle_both_orders','dust1','deficit1','max8fills16orders','limit_plus1','out_of_gas_rollback'],'T06,T15(partial)'),
 ('S3-AT04','B/D/G/J',['same_TxRaw','same_batch_new_TX','past_same','past_different','past_missing','fill_duplicate','seq_gap','prev_wrong','operator_rotation','close_then_late_settle','old_commit_after_rotation'],'T07,T02,T10(partial)'),
 ('S3-AT05','D/F/G/J',['F05','F06','F07','F08','temporary_NOT_FOUND','query_failure','receipt_mismatch','expired_absence_scan_gap','budget_exhausted','worker_restart'],'T09'),
 ('S3-AT06','B/C/D/F/G/J',['withdraw_full_then_settle','withdraw_atom_then_settle','settle_then_full_withdraw_failure','settle_then_atom','settle_then_remaining','same_block_both_tx_index_orders'],'T04'),
 ('S3-AT07','C/D/F/G/J',['F13','F14','F15','F16','dependency_shared_order','dependency_shared_reservation','same_owner_independent_asset','committed_immutable','1000fills200orders_fee0','1001fills201orders_fee0','1000fills200orders_fee25','1001fills201orders_fee25'],'T16'),
 ('S3-AT08','C/D/F/G/J',['F01','F02','F03','F04','F09','F10','F11','F12','F17','F18','HTTP12_new','HTTP12_identical_retry','cancel_then_match','match_then_cancel'],'T03,T05(partial),T08(local only)'),
 ('S3-AT09','E/H/I/J',['fresh_main_checkout_CI','browser_guide_verbatim','stale','disconnect','height_regression','revision_duplicate','revision_conflict','revision_gap','account_switch','explicit_withdraw_abort','artifact_access'],'T14(REST/UI only)')]
write('acceptance.json',{'scenario_repetitions':'3','deterministic_replays':'2','mutation_detection_required':True,
 'cases':[dict(id=i,owners=o,variants=v,legacy_trace=t,a_product_result='NOT_RUN') for i,o,v,t in matrix],
 'evidence_required':['code_sha','tree_sha','contract_hash','config_hash','vector_hash','lock_hashes','runtime_genesis_hash','versions','commands','height','tx_index','tx_hash','batch_seq','batch_id','batch_hash','fill_ids','raw_inputs','raw_outputs','expected_actual_diff','PASS_FAIL_NOT_RUN'],
 'timings_separate':['request_to_headers_ms','request_to_complete_JSON_ms','request_to_chain_commit_ms','commit_to_engine_apply_ms'],
 'legacy_full_pass_at_start':'0/16','excluded':['real_assets','distributed_ACK','AZ_loss','distributed_fencing','independent_emergency_RPC_gas','transfer_sponsorship','full_WS','TPS_SLA']})
print('generated strict S3 schema, errors, 18 fault points and AT01..09 matrix')
