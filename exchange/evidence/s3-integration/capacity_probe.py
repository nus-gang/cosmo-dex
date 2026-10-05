#!/usr/bin/env python3
"""Pinned schema size experiment; synthetic RPC bytes, NOT a chain proof."""
import base64,copy,hashlib,json,pathlib,sys
root=pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0,str(root/'protocol/s3/tools'))
from check import validate
defs=json.loads((root/"protocol/s3/schema.json").read_text())["$defs"]
def validate_type(name,x): validate(x,defs[name],defs)
v=json.loads((root/'protocol/s3/vectors/correction-state-hash.json').read_text())
step=copy.deepcopy(v['steps'][0]); state=step['after_state']; result=step['result']; journal=step['journal_record']
canon=lambda x: json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()
b64=lambda x: base64.b64encode(x).decode()
# JSON-RPC whitespace is not bounded by consensus block_max_bytes. The required
# raw response bytes are preserved verbatim, rather than canonicalized/trimmed.
raw=b'{"jsonrpc":"2.0","id":1,"result":{"height":"201","txs_results":[]}}'
raw += b' '*(3*1024*1024-len(raw))
assert json.loads(raw)['result']['height']=='201'
for receipt in [state['resolution_receipts'][0],state['corrections'][0]['resolution_receipt'],result['correction_results'][0]['resolution_receipt']]:
    receipt['terminal_tx']['raw_results_response']=b64(raw)
def digest(domain,x):
    d=('NUS/S3/'+domain+'/V1').encode(); b=canon(x)
    return hashlib.sha256(len(d).to_bytes(4,'big')+d+len(b).to_bytes(8,'big')+b).hexdigest()
# Only structural/encoded size is tested. Recompute hashes consistently below;
# fixed fixture already labels itself synthetic, and no valid raw inclusion is claimed.
h=digest('ENGINE_STATE',state)
result['after_state_hash']=h
result['correction_results'][0]['after_state_hash']=h
journal['after_state_hash']=h
journal['state_json']=b64(canon(state));journal['result_json']=b64(canon(result));journal['result_hash']=digest('COMMAND_RESULT',result)
for name,x in [('EngineState',state),('CommandResult',result),('JournalRecord',journal)]: validate_type(name,x)
size=len(canon(journal)); limit=16_777_216
assert size>limit
out={'case':'raw-receipt-duplication','scope':'SCHEMA_AND_SERIALIZATION_ONLY_NOT_CHAIN_PROOF','approved_contract_head':'d45be33029705859b07a9516fd2229a56dc66f46','raw_response_bytes':len(raw),'raw_response_sha256':hashlib.sha256(raw).hexdigest(),'raw_response_schema_base64_chars':len(b64(raw)),'state_bytes':len(canon(state)),'result_bytes':len(canon(result)),'journal_payload_bytes':size,'payload_limit':limit,'over_limit_bytes':size-limit,'schema_validation':'PASS','payload_fit':'FAIL','invariant':'Before ACK, reserve complete worst correction with all original evidence. Cannot substitute refs inside ResolutionReceipt fields under s3/2 schema.'}
print(json.dumps(out,ensure_ascii=False,indent=2))
