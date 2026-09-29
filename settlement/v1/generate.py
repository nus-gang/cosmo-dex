"""Generate API schemas directly from the pinned A field definitions."""
import json
from pathlib import Path
from adapter import SCHEMA, ROOT, string_schema
HERE = Path(__file__).resolve().parent

def obj(props):
    return {'type':'object','additionalProperties':False,'required':list(props),'properties':props}

def uint(bits):
    # Exact range is additionally enforced by adapter.integer; lexical decimal schema.
    return {'type':'string','pattern':'^(0|[1-9][0-9]*)$','maxLength':len(str(2**bits-1)), 'x-maximum':str(2**bits-1)}

def generate():
    defs = {}
    for name, fields in SCHEMA.items():
        props = {}
        for f in fields:
            t=f['type']
            if t in ('u32','u64','atoms'): s=uint(128 if t=='atoms' else int(t[1:]))
            elif t=='h': s={'type':'string','pattern':'^[0-9a-f]{64}$'}
            elif t in ('a','pk','sig'): s={'type':'string','contentEncoding':'base64','x-decoded-length':{'a':20,'pk':1952,'sig':3309}[t]}
            elif t=='s': s=string_schema(f['name'])
            else: s={'$ref':'#/$defs/'+t}
            props[f['name']]={'type':'array','items':s} if f['repeated'] else s
        defs[name]=obj(props)
    defs['Event']=obj({'entity_id':{'type':'string'},'revision':uint(64),'observed_height':uint(64),
                       'state':{'enum':['PENDING','COMMITTED','CORRECTED','SUBMISSION_UNKNOWN']}})
    defs['Lookup']=obj({'code':{'enum':['COMMITTED','NOT_FOUND_AT_HEIGHT','LOOKUP_UNAVAILABLE','RECEIPT_INCONSISTENCY']},
        'retryable':{'type':'boolean'},'state':{'enum':['COMMITTED','SUBMISSION_UNKNOWN']},
        'height':uint(64),'observed_height':uint(64),'indexer_height':uint(64),'stale':{'type':'boolean'},
        'receipt':{'anyOf':[{'$ref':'#/$defs/BatchReceiptV1'},{'type':'null'}]}})
    schema={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'urn:nus:settlement:api:v1', '$defs':defs}
    (HERE/'api.schema.json').write_text(json.dumps(schema,indent=2)+'\n')
    source=json.loads((ROOT/'protocol/v1/vectors/message-codec.json').read_text())
    receipts=[v['api_json'] for v in source['positives'] if v['message']=='BatchReceiptV1' and 'source_batch_id' in v]
    # One canonical receipt per key; mutation vectors are used by tests separately.
    seen={}
    for r in receipts: seen.setdefault(tuple(r[k] for k in ('chain_id','genesis_hash','market_id','batch_seq')),r)
    fixtures={'receipts':list(seen.values()),'events':[{'entity_id':'synthetic-order-1','revision':str(i+1),'observed_height':'100','state':s} for i,s in enumerate(['PENDING','SUBMISSION_UNKNOWN','CORRECTED'])]}
    fixtures['events'].append({'entity_id':'synthetic-order-2','revision':'1','observed_height':'100','state':'COMMITTED'})
    (HERE/'fixtures.json').write_text(json.dumps(fixtures,indent=2)+'\n')
if __name__=='__main__': generate()
