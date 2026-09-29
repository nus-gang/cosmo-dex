# Apply rc4 full-output semantics to historical binding IDs, retaining their IDs.
for row in rows:
 ident=row['id'];policy=None;sid='synthetic-1'
 if ident.startswith('rc3-binding-'):
  policy='NOT_CONNECTED';sid='other' if ident=='rc3-binding-id' else sid
 elif ident=='rc3-epoch-flag-binding':policy='NOT_CONNECTED'
 elif ident.startswith('fixed-epoch-'):
  policy={'fixed-epoch-True-True':'OK','fixed-epoch-False-False':'EPOCH_MISMATCH'}.get(ident,'NOT_CONNECTED')
 if policy is not None:
  row['expected']=expect_decision(policy=policy,sid=sid)
  row['passed']=row['actual']==row['expected']
# Map specification projection to genuinely encoded/signed OrderV1 requests.
rc4=json.loads((ROOT/'protocol/v1/vectors/snapshot-output.json').read_text())
for v in rc4['cases']:
 inp=v['input'];auth=inp['authentication_result']['status']
 extra={'BadSig':True} if auth=='REJECTED' else {'MissingKeyType':True} if auth=='NOT_CONNECTED' else {}
 r=signed_request(inp['authenticated_order'],extra)
 if 'snapshot' in inp:r['Snapshot']=copy.deepcopy(inp['snapshot'])
 else:r.pop('Snapshot',None)
 r['Observation']=copy.deepcopy(inp['context'])
 # Go's actual Context JSON unmarshaller preserves omitted/null numeric fields.
 for original,key in [('snapshot_id','SnapshotID'),('height','Height'),('epoch','Epoch')]:
  r['C'].pop(key,None)
  if original in inp['context']:
   value=inp['context'][original]
   r['C'][key]=int(value) if key!='SnapshotID' and value is not None else value
 expected=dict(code='OK',decision=v['expected'])
 policy_inputs.append(dict(id=v['id'],request=r,expected=expected))
 for lang in ps:check(v['id'],lang,r,expected)
# Independent regression for null/omitted epoch with a false flag: absence is not
# evidence of an epoch mismatch. Unlike the 60 vectors this exercises false.
for absent in ['null','omitted']:
 r=signed_request(snapshot_updates={'epoch_matches':False})
 r['Observation']={'snapshot_id':'synthetic-1','height':'999'}
 r['C'].pop('Epoch')
 if absent=='null':r['Observation']['epoch']=None;r['C']['Epoch']=None
 expected=expect_decision(policy='NOT_CONNECTED')
 ident='rc4-extra-epoch-'+absent+'-false'
 policy_inputs.append(dict(id=ident,request=r,expected=expected))
 for lang in ps:check(ident,lang,r,expected)
