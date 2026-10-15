"""Real initialize/checker/create rejection; synthetic fetch and approval.

No RPC, START, home creation, or service start. Rust supplies fee0/25 inputs.
"""
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
from types import SimpleNamespace
import bootstrap_initialize
import bootstrap_fetch_cli


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
    args = SimpleNamespace(bundle=bundle, artifacts=artifacts, scratch=scratch,
        runtime_pin=pin, local_demo_profile='s3-dev-local/1',
        acknowledge_unproven_space=True, input_set=root/'input.json',
        effective_profile=root/'profile')
    popen = ready_worker.subprocess.Popen
    children = []
    def spawn(*argv, **kwargs):
        child = popen(*argv, **kwargs)
        if 'create-captured' in argv[0]:
            children.append(child)
        return child
    for scenario in ('revoked-before-start', 'stopped-after-ready'):
        evidence = root / scenario
        evidence.mkdir(mode=0o700)
        extra = SimpleNamespace(evidence_root=evidence, chain_rpc='127.0.0.1:26657')
        audits_after_ready = []
        stopped = [False]
        def audit(*unused):
            # The child's durable evidence precedes READY; observing it at the
            # parent's audit proves that the real child reached that boundary.
            if (evidence/'bootstrap-rpc.json').exists():
                audits_after_ready.append(1)
                if len(audits_after_ready) == 2:
                    if scenario == 'revoked-before-start':
                        return {'revoked': True}
                    stopped[0] = True
            return {'synthetic': 'same'}
        with patch('approval_gate.inspect', side_effect=audit), \
             patch.object(bootstrap_fetch_cli, 'fetch', return_value=rpc) as fetch, \
             patch('ready_worker.subprocess.Popen', side_effect=spawn):
            try:
                bootstrap_initialize.initialize(args, 'synthetic', {}, extra, home,
                                                stopped=lambda: stopped[0])
            except ValueError as error:
                expected = ('INITIALIZE_APPROVAL_CHANGED' if scenario == 'revoked-before-start'
                            else 'BOOTSTRAP_STOPPED')
                assert str(error) == expected, (scenario, str(error))
            else:
                raise AssertionError('creation must be refused')
            assert fetch.call_count == 1
        assert len(audits_after_ready) == 2
        assert len(children) == (1 if scenario == 'revoked-before-start' else 2)
        child = children[-1]
        assert child.poll() is not None
        try:
            os.waitpid(child.pid, os.WNOHANG)
        except ChildProcessError:
            pass
        else:
            raise AssertionError('child not reaped')
        assert not home.exists()
        assert (evidence/'snapshot-fetch.raw').read_bytes() == rpc
        assert (evidence/'bootstrap-rpc.json').read_bytes() == rpc
        assert (evidence/'snapshot-fetch.raw').stat().st_mode & 0o777 == 0o600
        assert list(scratch.iterdir()) == []
        assert (root/'input.json').read_bytes() == original
    print(json.dumps({'home_created': False, 'ready': True, 'start_sent': False,
                      'rpc_attempted': False, 'approval_verified': False}))

if __name__ == '__main__':
    main()
