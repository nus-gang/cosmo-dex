"""Independent retry/state tests against the submitted synthetic adapter."""
import sys,pathlib,json,itertools,copy
root=pathlib.Path(__file__).resolve().parents[1];sys.path.insert(0,str(root/'settlement/v1'))
from adapter import MockAPI,reconcile,Attempt,available,EventConsumer
vectors=json.loads((root/'protocol/v1/vectors/message-codec.json').read_text())
r=next(v['api_json'] for v in vectors['positives'] if v['id']=='receipt');api=MockAPI([r]);out=[]
def check(id,actual,expected):
 assert actual==expected,(id,actual,expected)
 out.append(dict(id=id,actual=actual,expected=expected,passed=True))
check('historical-retry-after-epoch-change',api.retry(r,str(int(r['batch_seq'])+100)),'ALREADY_COMMITTED')
check('current-operator-auth-required',api.retry(r,str(int(r['batch_seq'])+100),authorized=False),'UNAUTHORIZED')
for field in ['batch_id','batch_hash']:
 x=copy.deepcopy(r);x[field]='ab'*32
 check('historical-conflict-'+field,api.retry(x,str(int(r['batch_seq'])+100)),'BATCH_CONFLICT')
args=[r[k] for k in ['chain_id','genesis_hash','market_id','batch_seq']]
for flags in itertools.product([False,True],repeat=3):
 result=reconcile({'code':'NOT_FOUND_AT_HEIGHT'},rejected_final=flags[0],inflight_resolved=flags[1],replay_complete=flags[2])
 check('release-prerequisites-'+str(flags),result['release_D_P'],all(flags))
for code in ['LOOKUP_UNAVAILABLE','NOT_FOUND_AT_HEIGHT','RECEIPT_INCONSISTENCY']:
 check('uncertain-holds-'+code,reconcile({'code':code})['release_D_P'],False)
check('confirmed-receipt-wins-over-correction',reconcile({'code':'COMMITTED'},rejected_final=True,inflight_resolved=True,replay_complete=True)['state'],'COMMITTED')
check('provisional-is-not-spendable',available('100','30','40',str(2**128-1)),'30')
a=Attempt(r['batch_id'],b'original');a.timeout();check('same-bytes-retry',a.retry(r['batch_id'],b'original',lookup_completed=True),b'original')
for id,payload,queried in [('changed-bytes',b'changed',True),('no-lookup',b'original',False)]:
 try:a.retry(r['batch_id'],payload,lookup_completed=queried);raise AssertionError(id)
 except ValueError as e:check(id,str(e),'RETRY_BINDING')
# serialize without binary values
for x in out:
 for k in ['actual','expected']:
  if isinstance(x[k],bytes):x[k]=x[k].hex()
(root/'security/evidence/settlement-independent.json').write_text(json.dumps(out,indent=2)+'\n');print('PASS',len(out),'synthetic adapter checks; chain/DB NOT_CONNECTED')
