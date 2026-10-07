"""Real checker/fetch rejection; synthetic approval and corrupted transport.

Never sends valid capture to the fetch executable: no RPC or service start.
The Rust fixture supplies fee0/25 manifests pinned to actual executable bytes.
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
from unittest.mock import patch
import bootstrap_check
import bootstrap_fetch
import fetch_process


def main():
    root, checker, fetcher = map(Path, sys.argv[1:])
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
    (artifacts / 'bin').mkdir()
    shutil.copyfile(checker, artifacts / bootstrap_check.CHECKER)
    shutil.copyfile(fetcher, artifacts / bootstrap_fetch.FETCHER)
    pin = hashlib.sha256(manifest).hexdigest()

    def run():
        return bootstrap_fetch.fetch(bundle, artifacts, pin, 's3-dev-local/1', True,
            'synthetic', {}, root, 'input.json', root / 'profile',
            '127.0.0.1:26657', scratch, timeout=30)

    # The real C checker runs; revoked approval must prevent fetch child spawn.
    with patch('approval_gate.inspect', side_effect=[{}, {'revoked': True}]), \
         patch.object(bootstrap_fetch, 'fetch_captured') as child:
        try:
            run()
        except ValueError as error:
            assert str(error) == 'APPROVAL_CHANGED_DURING_FETCH', str(error)
        else:
            raise AssertionError('revoked approval accepted')
        child.assert_not_called()
    assert list(scratch.iterdir()) == []

    # Corrupt only the bytes at the private child boundary. Recompute transport
    # SHA through the real supervisor so rejection exercises C semantic checks.
    # An empty guard is unconditionally invalid before bootstrap::fetch is called.
    real_fetch = fetch_process.fetch_captured
    popen = fetch_process.subprocess.Popen
    children = []
    calls = []
    def spawn(*args, **kwargs):
        process = popen(*args, **kwargs)
        children.append(process)
        return process
    def corrupt(executable, address, pin, profile, ack, raw, timeout, stopped):
        assert raw == original
        assert executable.read_bytes() == fetcher.read_bytes()
        assert executable.stat().st_mode & 0o777 == 0o500
        assert executable.parent.stat().st_mode & 0o777 == 0o700
        changed = json.loads(raw)
        changed['guard'] = base64.b64encode(b'{}').decode()
        calls.append(1)
        with patch.object(fetch_process.subprocess, 'Popen', side_effect=spawn):
            return real_fetch(executable, address, pin, profile, ack,
                              json.dumps(changed).encode(), timeout, stopped)
    with patch('approval_gate.inspect', return_value={}) as audit, \
         patch.object(bootstrap_fetch, 'fetch_captured', side_effect=corrupt):
        try:
            run()
        except fetch_process.FetchFailure as error:
            assert str(error) == 'FETCH_REJECTED', str(error)
            assert error.partial_raw == b''
        else:
            raise AssertionError('invalid guard accepted by real fetch child')
        assert audit.call_count == 2
    assert calls == [1] and len(children) == 1
    assert children[0].returncode == 2
    try:
        os.waitpid(children[0].pid, os.WNOHANG)
    except ChildProcessError:
        pass
    else:
        raise AssertionError('fetch child not reaped')
    assert list(scratch.iterdir()) == []
    assert (root / 'input.json').read_bytes() == original
    print(json.dumps({'home_created': False, 'rpc_attempted': False,
                      'approval_verified': False, 'fetch_child_reaped': True}))


if __name__ == '__main__':
    main()
