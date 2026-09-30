#!/usr/bin/env python3
"""CLI rejection must precede any node-home writes; fixtures contain public data only."""
import argparse, base64, json, pathlib, subprocess
p=argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--operator-accounts', required=True)
p.add_argument('--output', required=True)
a=p.parse_args()
out=pathlib.Path(a.output).resolve(); out.mkdir(parents=True, exist_ok=False)
binary=str(pathlib.Path(a.binary).resolve())
ops=str(pathlib.Path(a.operator_accounts).resolve())
def init(home, source=None, operators=ops):
    args=[binary, 'init', '--home', str(home), '--operator-accounts', operators]
    if source is not None: args += ['--user-public-keys', str(source)]
    return subprocess.run(args, capture_output=True, text=True, timeout=30)
fixture=out/'fixture'
r=init(fixture); assert r.returncode==0, r.stderr
keys=json.loads((fixture/'config/genesis.json').read_text())['app_state']['public_keys']
cases={'null':'null', 'object':json.dumps({'public_keys':keys}), 'one':json.dumps(keys[:1]),
       'three':json.dumps(keys+[keys[0]]), 'duplicate':json.dumps([keys[0]]*2),
       'number':json.dumps([1, keys[1]]), 'null_key':json.dumps([None, keys[1]]),
       'trailing':json.dumps(keys)+' null', 'malformed':'[',
       'secret_field':json.dumps({'seed':'DO_NOT_ECHO_SENTINEL','public_keys':keys})}
for name, key in {'short':base64.b64encode(bytes(1951)).decode(), 'long':base64.b64encode(bytes(1953)).decode(),
                  'newline':keys[0][:30]+'\n'+keys[0][30:], 'unpadded':keys[0].rstrip('='),
                  'invalid':'DO_NOT_ECHO_SENTINEL', 'padding_bits':base64.b64encode(bytes(1952)).decode()[:-2]+'B='}.items():
    cases[name]=json.dumps([key, keys[1]])
results=[]
for name, body in cases.items():
    source=out/(name+'.json'); source.write_text(body)
    home=out/(name+'-home'); r=init(home, source)
    assert r.returncode!=0 and not home.exists(), name
    assert 'DO_NOT_ECHO_SENTINEL' not in r.stdout+r.stderr
    results.append({'case':name, 'rejected':True, 'home_absent':True})
for name, source in [('empty_path', ''), ('missing_file', out/'absent.json')]:
    home=out/(name+'-home'); r=init(home, source)
    assert r.returncode!=0 and not home.exists()
    results.append({'case':name, 'rejected':True, 'home_absent':True})
source=out/'valid.json'; source.write_text(json.dumps(keys))
home=out/'valid-home'; r=init(home, source); assert r.returncode==0, r.stderr
assert json.loads(r.stdout)['users']==json.loads(init(out/'fixture2').stdout)['users']
operators=json.loads(pathlib.Path(ops).read_text()); operators[0]['address']=json.loads(r.stdout)['users'][0]
collision=out/'collision.json'; collision.write_text(json.dumps(operators))
home=out/'collision-home'; r=init(home, source, str(collision))
assert r.returncode!=0 and not home.exists()
results.append({'case':'operator_user_collision','rejected':True,'home_absent':True})
print(json.dumps({'status':'PASS','rejected_cases':len(results),'fixture_and_public_input_match':True,'results':results},indent=2))
