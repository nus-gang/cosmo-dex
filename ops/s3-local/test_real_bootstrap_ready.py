"""Rust-owned fee0/25 fixture; real checker, synthetic organizational audit."""
import base64
import hashlib
import json
from pathlib import Path
import shutil
import sys
from unittest.mock import patch
import bootstrap_check
import bootstrap_stage
import bootstrap_ready
import bootstrap_run
import ready_worker
import os


def main():
    root, executable, creator = map(Path, sys.argv[1:])
    original = (root / 'input.json').read_bytes()
    capture = json.loads(original)
    bundle, artifacts, scratch = (root / x for x in ('bundle', 'artifacts', 'scratch'))
    for path in (bundle, artifacts, scratch):
        path.mkdir(mode=0o700)
    manifest = base64.b64decode(capture['runtime_manifest'], validate=True)
    (bundle / 'runtime-manifest.json').write_bytes(manifest)
    for name, raw in capture['files'].items():
        path = bundle / 'files' / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(base64.b64decode(raw, validate=True))
    target = artifacts / bootstrap_check.CHECKER
    target.parent.mkdir()
    shutil.copyfile(executable, target)
    shutil.copyfile(creator, artifacts / bootstrap_stage.CREATOR)
    rpc = b'{"synthetic":"raw RPC preserved; no START"}'
    pin = hashlib.sha256(manifest).hexdigest()
    home = root / 'new-home'
    popen = ready_worker.subprocess.Popen
    children = []
    def spawn(*args, **kwargs):
        child = popen(*args, **kwargs)
        children.append(child)
        return child
    for scenario in ("ready-denied", "ready-revoked", "start-revoked", "start-rpc-changed", "start-stopped"):
        revoke = scenario == "ready-revoked"
        evidence = root / ("evidence-" + scenario)
        evidence.mkdir(mode=0o700)
        with patch('approval_gate.inspect', return_value={'synthetic': 'same'}):
            with bootstrap_stage.stage(bundle, artifacts, pin, 's3-dev-local/1', True,
                    'synthetic', {}, root, 'input.json', root / 'profile', rpc, scratch) as staged:
                calls = []
                def audit():
                    calls.append(1)
                    if len(calls) == 3:
                        if scenario == 'start-revoked':
                            return 'changed'
                        if scenario == 'start-rpc-changed':
                            staged.rpc_file.write_bytes(b'changed')
                    return 'changed' if revoke and len(calls) == 2 else 'same'
                with patch('ready_worker.subprocess.Popen', side_effect=spawn):
                    try:
                        if scenario.startswith('start-'):
                            bootstrap_run.run(staged, home, evidence, pin, root / 'profile', audit,
                                stop=lambda: scenario == 'start-stopped' and len(calls) == 3, timeout=30)
                            raise AssertionError('create must be denied')
                        with bootstrap_ready.ready(staged, home, evidence, pin,
                                root / 'profile', audit, timeout=30) as report:
                            assert not revoke
                            assert report['ready'] and not home.exists()
                            assert (evidence / 'bootstrap-rpc.json').read_bytes() == rpc
                    except ValueError as error:
                        expected = {'ready-revoked': 'APPROVAL_CHANGED_AFTER_READY',
                            'start-revoked': 'APPROVAL_CHANGED_BEFORE_CREATE',
                            'start-rpc-changed': 'BOOTSTRAP_RPC_CHANGED',
                            'start-stopped': 'BOOTSTRAP_STOPPED'}
                        assert str(error) == expected.get(scenario), (scenario, str(error))
                assert len(calls) == (3 if scenario.startswith('start-') else 2)
                assert children[-1].poll() is not None
                try:
                    os.waitpid(children[-1].pid, os.WNOHANG)
                except ChildProcessError:
                    pass
                else:
                    raise AssertionError('child not reaped')
                assert not home.exists()
                assert (evidence / 'bootstrap-rpc.json').read_bytes() == rpc
        assert list(scratch.iterdir()) == []
    print(json.dumps(report))

if __name__ == '__main__':
    main()
