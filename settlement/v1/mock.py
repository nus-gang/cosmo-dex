"""Offline REST response / WS frame generator for Wallet and QA."""
import json
import sys
from pathlib import Path
from urllib.parse import urlsplit, parse_qs, unquote
from adapter import MockAPI, KEYS

def rest(api, url):
    u=urlsplit(url); parts=u.path.split('/')
    q=parse_qs(u.query, strict_parsing=True)
    if len(parts)!=6 or parts[1:3]!=['v1','markets'] or parts[4]!='batches':
        raise ValueError('ROUTE')
    if set(q)!={'chain_id','genesis_hash'} or any(len(v)!=1 for v in q.values()):
        raise ValueError('QUERY')
    result=api.lookup(q['chain_id'][0],q['genesis_hash'][0],unquote(parts[3]),parts[5])
    return (200 if result['code']=='COMMITTED' else 409 if result['code']=='RECEIPT_INCONSISTENCY' else 404),result

if __name__=='__main__':
    f=json.loads(Path(__file__).with_name('fixtures.json').read_text())
    if len(sys.argv)==2 and sys.argv[1]=='ws':
        for event in f['events']: print(json.dumps(event))
    else:
        api=MockAPI(f['receipts']); r=f['receipts'][0]
        url=sys.argv[1] if len(sys.argv)>1 else f"/v1/markets/{r['market_id']}/batches/{r['batch_seq']}?chain_id={r['chain_id']}&genesis_hash={r['genesis_hash']}"
        status,body=rest(api,url)
        print(json.dumps({'http_status':status,'body':body},indent=2))
