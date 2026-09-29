"""Targeted implementation regression, not the independent S0-G full verdict.
Usage: python3 exchange/tools/compare_admission.py <materialized cross root>
Cross root: pinned chain/web/protocol plus security/go.go, runner.ts;
compile go.go to security/go-runner and install locked web dependencies.
"""
import base64, json, pathlib, subprocess, sys
root = pathlib.Path(sys.argv[1]).resolve()
evidence = pathlib.Path(__file__).resolve().parents[1] / 'evidence/admission'
inputs = json.loads((evidence/'inputs.json').read_text())
rust = json.loads((evidence/'rust-results.json').read_text())
commands = {'Go': [str(root/'security/go-runner')], 'TS': ['node','--experimental-strip-types',str(root/'security/runner.ts')]}
processes = {k:subprocess.Popen(v, cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) for k,v in commands.items()}
def call(lang, req):
    p=processes[lang];p.stdin.write(json.dumps(req)+'\n');p.stdin.flush()
    return json.loads(p.stdout.readline())
rows=[]
for req, result in zip(inputs,rust):
    if req['id']=='missing-epoch': continue  # Rust CLI schema-specific regression
    c=req['context'];pk=c['registered']['raw_key_hex']
    api=call('Go',dict(Op='decode',Name='OrderV1',Wire=req['wire_hex']))['api']
    context=dict(SnapshotID=c['snapshot_id'],ChainID=c['chain_id'],GenesisHash=c['genesis_hash'],ModuleID=c['exchange_module_id'],MarketID=c['market_id'],MarketConfigVersion=c['market_config_version'],RegisteredKey=base64.b64encode(bytes.fromhex(pk)).decode(),RegisteredKeyType='ML-DSA-65',Height=int(c['height']),Epoch=int(c['epoch']))
    common=dict(Op='decision',Name='OrderV1',API=api,Wire=req['wire_hex'],Sig=req['signature_hex'],PK=pk,SnapshotID=c['snapshot_id'],Height=int(c['height']),Epoch=int(c['epoch']),C=context,Snapshot=req['snapshot'])
    for lang,out in [('Rust',result),*[(lang,call(lang,common)['decision']) for lang in processes]]:
        policy=out['snapshot_policy'];expected=req['expected']
        ok=out['authentication']['status']=='PASS' and out['ack']=='NOT_CONNECTED' and out['ledger']=='NOT_CONNECTED' and out['wal_replay']=='NOT_RUN'
        if expected=='NOT_CONNECTED':ok &= policy['status'] in ('REJECTED','NOT_CONNECTED')
        else:ok &= policy['code']==expected and policy['status']==('PASS' if expected=='OK' else 'REJECTED')
        rows.append(dict(id=req['id'],language=lang,expected=expected,passed=ok,actual=out))
for p in processes.values():p.stdin.close();p.wait()
(evidence/'cross-results.json').write_text(json.dumps(rows,indent=2)+'\n')
summary=dict(comparisons=len(rows),passed=sum(r['passed'] for r in rows),failures=[dict(id=r['id'],language=r['language']) for r in rows if not r['passed']],scope='targeted implementation regression; full independent retest NOT_RUN')
(evidence/'cross-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
sys.exit(1 if summary['failures'] else 0)
