"""Invoked only by the Rust fee0/25 fixture: no production keys or approval."""
import base64
import hashlib
import json
import os
from unittest.mock import patch
from pathlib import Path
import shutil
import sys
from offline_check import check


def main():
    root, executable = map(Path, sys.argv[1:3])
    capture = json.loads((root / 'input.json').read_bytes())
    bundle, artifacts, scratch = [root / x for x in ('bundle', 'artifacts', 'scratch')]
    for path in (bundle, artifacts, scratch):
        path.mkdir(mode=0o700)
    manifest = base64.b64decode(capture['runtime_manifest'], validate=True)
    (bundle / 'runtime-manifest.json').write_bytes(manifest)
    for name, raw in capture['files'].items():
        path = bundle / 'files' / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(base64.b64decode(raw, validate=True))
    (artifacts / 'web').mkdir()
    (artifacts / 'web/index.html').write_bytes(b'<!doctype html><title>synthetic web fixture</title>')
    (artifacts / 'web/page.js').write_bytes(b'// synthetic web fixture')
    (artifacts / 'bin').mkdir()
    shutil.copyfile(executable, artifacts / 'bin/s3-local-preflight')
    worker = os.environ.get('NUS73_READY_WORKER_EXECUTABLE')
    if worker:
        shutil.copyfile(worker, artifacts / 'bin/s3-local-worker')
        from staged_direct import HELPER
        shutil.copyfile(os.environ['NUS73_DIRECT_HELPER_EXECUTABLE'], artifacts / HELPER)
    fault = os.environ.get('NUS73_STORAGE_FAULT_EXECUTABLE')
    if fault:
        from storage_fault_stage import FAULT
        shutil.copyfile(fault, artifacts / FAULT)
    result = check(bundle, artifacts, hashlib.sha256(manifest).hexdigest(),
                   's3-dev-local/1', True, root, 'input.json', sys.argv[3:], scratch)
    assert list(scratch.iterdir()) == []
    assert result['byte_preflight']['input_set_byte_match'] is True
    assert result['semantic_preflight']['durable_ack'] is False
    if worker:
        from staged_worker import checked_ready
        # This fixture substitutes only organizational approval. Real descriptor,
        # capture, validator, Engine, signer, worker READY and cleanup are used.
        for mode in ('ready', 'revoke_after_ready'):
            calls = []
            def audit(*args):
                calls.append(1)
                if mode == 'revoke_after_ready' and len(calls) == 9:
                    raise ValueError('SYNTHETIC_REVOCATION_AFTER_READY')
                return {'fixture': 'same-candidate'}
            try:
                with patch('approval_gate.inspect', side_effect=audit):
                    with checked_ready(bundle, artifacts, hashlib.sha256(manifest).hexdigest(),
                            's3-dev-local/1', True, 'synthetic-decision', {}, root,
                            'input.json', sys.argv[3:], scratch) as report:
                        assert mode == 'ready'
                        assert report == {'ready': True, 'service_started': False,
                                          'approval_verified': False, 'reusable_permit': False}
            except ValueError as error:
                assert mode == 'revoke_after_ready' and str(error) == 'SYNTHETIC_REVOCATION_AFTER_READY'
            assert len(calls) == 9, calls
            assert list(scratch.iterdir()) == []
        result['real_worker_ready'] = True
        result['synthetic_approval_only'] = True
    if fault:
        from test_real_storage_fault import verify
        verify(root, bundle, artifacts, scratch, manifest, sys.argv[3:])
        result['real_storage_fault_ready'] = True
    from writer_release import check as check_writer
    from preflight import verify_input_set
    raw, _ = verify_input_set(bundle, artifacts, hashlib.sha256(manifest).hexdigest(),
        's3-dev-local/1', True, root, 'input.json')
    writer = check_writer(raw, artifacts, sys.argv[3:], scratch)
    assert writer['writer_reopen_verified'] is True
    assert writer['process_exit_verified'] is False
    assert writer['commit_unchanged_verified'] is False
    assert list(scratch.iterdir()) == []
    result['writer_probe'] = writer
    from reviewed_web import prepare
    pin = hashlib.sha256(manifest).hexdigest()
    with patch('approval_gate.inspect', return_value={'synthetic': 'same-candidate'}) as audit:
        prepared = prepare(bundle, artifacts, pin, 's3-dev-local/1', True,
            'synthetic-decision', {}, root, 'input.json', sys.argv[3:], scratch,
            'http://127.0.0.1:5173')
        assert audit.call_count == 2
    headers = [('Host', '127.0.0.1:5173')]
    respond = prepared.response_boundary.respond
    context = json.loads(base64.b64decode(capture['guard']))['context']
    status, _, body = respond('GET', '/runtime-context.json', headers, '127.0.0.1')
    assert status == 200 and json.loads(body) == context
    assert prepared.capture_sha256 == hashlib.sha256(raw).hexdigest()
    assert prepared.validator_sha256 == hashlib.sha256(executable.read_bytes()).hexdigest()
    (artifacts / 'web/page.js').write_bytes(b'replaced after validation')
    assert respond('GET', '/page.js', headers, '127.0.0.1')[2] == b'// synthetic web fixture'
    (artifacts / 'web/page.js').write_bytes(b'// synthetic web fixture')
    # Invalid C guard is captured faithfully but must fail semantic validation.
    original = (root / 'input.json').read_bytes()
    changed = json.loads(original)
    guard = json.loads(base64.b64decode(changed['guard']))
    guard['context']['genesis_hash'] = '0' * 64
    changed['guard'] = base64.b64encode(json.dumps(guard).encode()).decode()
    (root / 'input.json').write_text(json.dumps(changed))
    try:
        with patch('approval_gate.inspect', return_value={'synthetic': 'same-candidate'}) as audit:
            try:
                prepare(bundle, artifacts, pin, 's3-dev-local/1', True,
                    'synthetic-decision', {}, root, 'input.json', sys.argv[3:], scratch,
                    'http://127.0.0.1:5173')
            except ValueError as error:
                assert str(error) == "VALIDATOR_REJECTED", str(error)
                assert audit.call_count == 1
            else:
                raise AssertionError('invalid C guard accepted')
    finally:
        (root / 'input.json').write_bytes(original)
    assert list(scratch.iterdir()) == []
    result['real_web_c_preparation'] = True
    result['web_assets_synthetic'] = True
    result['synthetic_approval_only'] = True

    print(json.dumps(result))


if __name__ == '__main__':
    main()
