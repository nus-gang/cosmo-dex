"""Rust-owned fee0/25 fixture; real checker, synthetic organizational audit."""
import base64
import hashlib
import json
from pathlib import Path
import shutil
import sys
from unittest.mock import patch
import bootstrap_check


def main():
    root, executable = map(Path, sys.argv[1:])
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
    def check():
        return bootstrap_check.check(bundle, artifacts, hashlib.sha256(manifest).hexdigest(),
            's3-dev-local/1', True, 'synthetic-decision', {}, root, 'input.json',
            root / 'profile', scratch)
    with patch('approval_gate.inspect', return_value={'synthetic': 'same-candidate'}) as audit:
        result = check()
        assert audit.call_count == 2
    assert result['checker_sha256'] == hashlib.sha256(executable.read_bytes()).hexdigest()
    assert result['semantic_preflight']['semantic_validation'] is True
    assert result['home_created'] is False and result['approval_verified'] is False
    assert list(scratch.iterdir()) == []
    changed = json.loads(original)
    guard = json.loads(base64.b64decode(changed['guard']))
    guard['context']['genesis_hash'] = '0' * 64
    changed['guard'] = base64.b64encode(json.dumps(guard).encode()).decode()
    (root / 'input.json').write_text(json.dumps(changed))
    try:
        with patch('approval_gate.inspect', return_value={}) as audit:
            try:
                check()
            except ValueError as error:
                assert str(error) == 'VALIDATOR_REJECTED', str(error)
                assert audit.call_count == 1
            else:
                raise AssertionError('invalid guard accepted')
    finally:
        (root / 'input.json').write_bytes(original)
    with patch('approval_gate.inspect', side_effect=[{'revision': 1}, {'revision': 2}]):
        try:
            check()
        except ValueError as error:
            assert str(error) == 'APPROVAL_CHANGED_DURING_PREFLIGHT'
        else:
            raise AssertionError('changed approval accepted')
    assert list(scratch.iterdir()) == []
    print(json.dumps(result))


if __name__ == '__main__':
    main()
