#!/usr/bin/env python3
"""Record candidate identity and public evidence, never validator/session secrets."""
import argparse
import hashlib
import json
import platform
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    def git(*args):
        return subprocess.check_output(['git', *args], cwd=ROOT, text=True).strip()
    paths = ['protocol/s2/CONTRACT.md', 'protocol/s2/profile.json', 'protocol/s2/manifest.json',
             'protocol/v1/dev-config.json', 'chain/app/go.mod', 'chain/app/go.sum',
             'exchange/Cargo.lock', 'web/package-lock.json']
    files = {x: digest(ROOT/x) for x in paths}
    for name in ('chain/app/bin/nusd', 'exchange/target/debug/exchange-s2'):
        if (ROOT/name).is_file():
            files[name] = digest(ROOT/name)
    evidence = {str(x.relative_to(a.output)): digest(x) for x in sorted(a.output.rglob('*'))
                if x.is_file() and x != a.output/'manifest.json' and 'scratch' not in x.relative_to(a.output).parts}
    versions = {}
    for name, cmd in [('node',['node','--version']), ('chain',[str(ROOT/'chain/app/bin/nusd'),'version'])]:
        try:
            versions[name] = subprocess.check_output(cmd, text=True, stderr=subprocess.STDOUT).strip()
        except (OSError, subprocess.CalledProcessError):
            versions[name] = 'unavailable'
    record = dict(schema=1, versions=versions, timestamp=datetime.now(timezone.utc).isoformat(),
                  code_sha=git('rev-parse', 'HEAD'), tree=git('rev-parse', 'HEAD^{tree}'),
                  tracked_changes=git('diff', '--stat', 'HEAD'), platform=platform.platform(),
                  python=platform.python_version(), files_sha256=files, evidence_sha256=evidence,
                  scope='single-host synthetic S2; LOCAL_FSYNC only; no settlement or main QA')
    (a.output/'manifest.json').write_text(json.dumps(record, indent=2)+'\n')

if __name__ == '__main__':
    main()
