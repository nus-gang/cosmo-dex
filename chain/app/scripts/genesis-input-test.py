#!/usr/bin/env python3
"""Verify strict operator input and no partial home on real CLI rejection."""
import argparse, json, pathlib, subprocess
p = argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--operator-accounts', required=True)
p.add_argument('--output', required=True)
a = p.parse_args()
binary = str(pathlib.Path(a.binary).resolve())
out = pathlib.Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=False)
raw = pathlib.Path(a.operator_accounts).read_text()
cases = {}
for field in ('address', 'gas_atoms'):
    for alias in (field.upper(), field.title()):
        for mode in ('alias_only', 'alias_first', 'alias_last'):
            replacement = json.dumps(alias) + ':'
            if mode == 'alias_first':
                replacement += 'null,' + json.dumps(field) + ':'
            if mode == 'alias_last':
                replacement = json.dumps(field) + ':null,' + replacement
            # Normalize whitespace while preserving duplicate members introduced below.
            base = json.dumps(json.loads(raw), separators=(',', ':'))
            cases[f'{field}-{alias}-{mode}'] = base.replace(json.dumps(field)+':', replacement, 1)
    escaped = '\\u%04x' % ord(field[0]) + field[1:]
    base = json.dumps(json.loads(raw), separators=(',', ':'))
    cases[field+'-escaped-duplicate'] = base.replace(json.dumps(field)+':', '"'+escaped+'":null,"'+field+'":', 1)
base = json.dumps(json.loads(raw), separators=(',', ':'))
cases['reviewer-gas'] = base.replace('"gas_atoms":', '"gas_atoms":"1","GAS_ATOMS":', 1)
cases['reviewer-address'] = base.replace('"address":', '"ADDRESS":"nus1bad","address":', 1)
results = []
for name, value in cases.items():
    source = out / (name+'.json')
    source.write_text(value)
    home = out / (name+'-home')
    r = subprocess.run([binary, 'init', '--home', str(home), '--operator-accounts', str(source)], capture_output=True, text=True, timeout=30)
    assert r.returncode != 0, (name, r.stdout, r.stderr)
    assert not home.exists(), name+' created partial home'
    results.append({'case':name, 'exit_code':r.returncode, 'home_exists':False, 'stderr':r.stderr})
(out/'results.json').write_text(json.dumps(results, indent=2))
print(json.dumps({'status':'PASS', 'rejected_cases':len(results), 'all_homes_absent':True}))
