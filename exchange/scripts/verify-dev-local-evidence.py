#!/usr/bin/env python3
"""Offline structural oracle; no services, key generation or store migration."""
import argparse, base64, hashlib, json, pathlib, struct
p=argparse.ArgumentParser();p.add_argument('evidence',type=pathlib.Path);a=p.parse_args()
checks=[]
def check(ok,label):
    if not ok: raise AssertionError(label)
    checks.append(label)
def sha(b):return hashlib.sha256(b).hexdigest()
def canonical(v):return json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=True).encode()
for home in sorted((a.evidence/'stores').iterdir()):
    wal=(home/'journal.dev.wal').read_bytes();marker=json.loads((home/'commit.dev.json').read_bytes());guard=json.loads((home/'profile.guard.json').read_bytes())
    # Fault evidence is preserved verbatim, never repaired or adopted as a store.
    if not home.name.startswith(('economic-','receipt-')):continue
    off=seq=0;previous='0'*64
    while off<len(wal):
        header=wal[off:off+72];check(len(header)==72,home.name+':header')
        check(header[:4]==b'S3D1',home.name+':dev magic')
        n=struct.unpack('>I',header[4:8])[0];check(n<=16777216,home.name+':payload cap')
        payload=wal[off+72:off+72+n];check(len(payload)==n,home.name+':payload length')
        check(hashlib.sha256(header[:40]).digest()==header[40:72] and hashlib.sha256(payload).digest()==header[8:40],home.name+':checksums')
        r=json.loads(payload);check(canonical(r)==payload,home.name+':canonical')
        seq+=1;check(r['command_seq']==str(seq) and r['previous_commit_hash']==previous,home.name+':chain')
        check(r['context']==guard['context'],home.name+':context')
        state_bytes=base64.b64decode(r['state_json'],validate=True);state=json.loads(state_bytes)
        result_bytes=base64.b64decode(r['result_json'],validate=True);result=json.loads(result_bytes)
        check(canonical(state)==state_bytes and canonical(result)==result_bytes,home.name+':state/result canonical')
        check(result['command_seq']==str(seq) and state['last_command_seq']==str(seq),home.name+':single revision')
        for account in state['accounts']:
            for row in account['ledger']:
                c,res,d,pend,avail=(int(row[k]) for k in ('C','R','D','P','A'))
                check(min(c,res,d,pend,avail)>=0 and avail==c-res-d,home.name+':A=C-R-D; P excluded')
        for ref in r['evidence_refs']:
            raw=(home/'objects'/'sha256'/ref['sha256']).read_bytes();desc=(home/'objects'/'sha256'/(ref['sha256']+'.ref')).read_bytes()
            check(len(raw)==int(ref['byte_length']) and sha(raw)==ref['sha256'] and desc==canonical(ref),home.name+':raw+descriptor')
        previous=sha(header+payload);off+=72+n
    check(marker=={'command_seq':str(seq),'record_hash':previous,'end_offset':str(off)},home.name+':marker complete')
for path in a.evidence.glob('*ledger-*.json'):
    v=json.loads(path.read_bytes());last=0
    for e in v['receipt_ledger']:
        last+=1;check(e['command_seq']==str(last),path.name+':receipt contiguous')
        r=e['receipt'];check(set(r)=={'envelope_version','profile_id','context','development_receipt','durable_ack','storage_assurance','command_result'},path.name+':receipt fields')
        check(r['envelope_version']=='s3-dev-local/1' and r['durable_ack'] is False and r['development_receipt']=='LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE' and r['storage_assurance']=='UNPROVEN_HOST_SPACE',path.name+':development receipt only')
        check(r['command_result']['command_seq']==str(last),path.name+':receipt revision')
    check(v['expected_diff']==[] and v['replay_runs']=='2',path.name+':replay comparison present')
print(json.dumps({'scope':'OFFLINE_COMPONENT_EVIDENCE_STRUCTURE_NOT_CHAIN_OR_HOST_DURABILITY','result':'PASS','checks':len(checks)},indent=2))
