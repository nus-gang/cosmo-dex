#!/usr/bin/env python3
"""L-T fresh input publisher. Authenticated audit before keys and publication.

No service, RPC, registration, C store, or reusable permit. Failed/partial
output is retained. Same-UID trust matches the existing managed launchers.
"""
import base64
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import sys
import tempfile
import time

import approval_gate
from bootstrap_stage import _write
from launcher_signal import stop_latch
from manifest import decode, encode
import native_review
from offline_cli import Parser
from preflight import bounded, checked_root, verify, MAX_MANIFEST

INITIALIZER = 'bin/nus-s3-local-initialize'
VALIDATOR = 'bin/nus-s3-local-demo'
ERROR = 'LOCAL_INITIALIZATION_REJECTED_PRESERVE_PARTIAL_OUTPUT'


def parse(argv):
    argv = list(argv)
    if not argv or argv.pop(0) != 'initialize-reviewed':
        raise ValueError('COMMAND')
    opts = [x for x in argv if x.startswith('--')]
    if len(opts) != len(set(opts)) or any('=' in x for x in opts):
        raise ValueError('ARGUMENTS')
    p = Parser(allow_abbrev=False, add_help=False)
    for name in ('bundle', 'artifacts', 'effective-profile', 'user-public-keys', 'output', 'scratch'):
        p.add_argument('--'+name, type=Path, required=True)
    for name in ('runtime-pin', 'local-demo-profile', 'native-decision-id', 'ceo-revision',
                 'cto-revision', 'fee-bps', 'run-uuid', 'genesis-time'):
        p.add_argument('--'+name, required=True)
    p.add_argument('--acknowledge-unproven-space', action='store_true')
    a = p.parse_args(argv)
    for name in ('bundle', 'artifacts', 'effective_profile', 'user_public_keys', 'output', 'scratch'):
        path = getattr(a, name)
        if not path.is_absolute() or '..' in path.parts:
            raise ValueError('PATH')
    if (a.local_demo_profile != 's3-dev-local/1' or not a.acknowledge_unproven_space
            or a.fee_bps not in ('0', '25') or not re.fullmatch('[0-9a-f]{64}', a.runtime_pin)):
        raise ValueError('INPUT')
    for key in ('native_decision_id', 'ceo_revision', 'cto_revision', 'run_uuid'):
        native_review._uuid(getattr(a, key))
    return a


@contextmanager
def stage(a, audit):
    baseline = audit()
    verify(a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile, True)
    raw = bounded(checked_root(a.bundle), 'runtime-manifest.json', MAX_MANIFEST)
    if hashlib.sha256(raw).hexdigest() != a.runtime_pin:
        raise ValueError('MANIFEST_CHANGED')
    manifest = decode(raw)
    files = {}
    for name, digest in manifest['files_sha256'].items():
        content = bounded(checked_root(a.bundle), 'files/'+name, 16*1024*1024)
        if hashlib.sha256(content).hexdigest() != digest:
            raise ValueError('CAPTURE_CHANGED')
        files[name] = base64.b64encode(content).decode()
    captures = {
        'source.json': (encode({'runtime_manifest': base64.b64encode(raw).decode(), 'files': files}), 0o600),
        'profile.json': (bounded(checked_root(a.effective_profile.parent), a.effective_profile.name, 1024*1024), 0o600),
        'users.json': (bounded(checked_root(a.user_public_keys.parent), a.user_public_keys.name, 16*1024), 0o600),
    }
    for component, artifact in (('chain', INITIALIZER), ('exchange', VALIDATOR)):
        descriptor = decode(base64.b64decode(files[manifest['components'][component]], validate=True))
        hashes = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
        binary = bounded(checked_root(a.artifacts), artifact, 512*1024*1024)
        if not binary or hashlib.sha256(binary).hexdigest() != hashes.get(artifact):
            raise ValueError('EXECUTABLE_NOT_PINNED')
        captures[Path(artifact).name] = (binary, 0o500)
    with tempfile.TemporaryDirectory(prefix='initializer-', dir=checked_root(a.scratch)) as temp:
        root = Path(temp)
        for name, (content, mode) in captures.items():
            _write(root/name, content, mode)
        def check():
            for name, (content, _) in captures.items():
                if bounded(root, name, len(content)) != content:
                    raise ValueError('STAGED_INPUT_CHANGED')
        if audit() != baseline:
            raise ValueError('APPROVAL_CHANGED')
        check()
        yield root, check, baseline


def run(a, root, check, baseline, audit, stopped=lambda: False):
    """Internal supervisor; caller must keep authenticated stage alive."""
    check()
    if stopped() or audit() != baseline:
        raise ValueError('APPROVAL_CHANGED')
    argv = [str(root/Path(INITIALIZER).name), 'create', '--source-input', str(root/'source.json'),
            '--effective-profile', str(root/'profile.json'), '--user-public-keys', str(root/'users.json'),
            '--c-validator', str(root/Path(VALIDATOR).name), '--scratch', str(root),
            '--output', str(a.output), '--runtime-pin', a.runtime_pin, '--fee-bps', a.fee_bps,
            '--run-uuid', a.run_uuid, '--genesis-time', a.genesis_time,
            '--publication-gate', 'stdin', '--local-demo-profile', 's3-dev-local/1',
            '--acknowledge-unproven-space']
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             env={'PATH': '/usr/bin:/bin', 'LANG': 'C', 'LC_ALL': 'C'},
                             close_fds=True, start_new_session=True)
    try:
        ready = b'INITIALIZER_READY\n'
        received, sent = bytearray(), False
        deadline = time.monotonic()+90
        with selectors.DefaultSelector() as select:
            for stream in (child.stdout, child.stderr):
                os.set_blocking(stream.fileno(), False)
                select.register(stream, selectors.EVENT_READ)
            while select.get_map():
                if stopped() or time.monotonic() >= deadline:
                    raise ValueError('INITIALIZER_STOPPED_OUTCOME_UNKNOWN')
                for key, _ in select.select(.05):
                    data = os.read(key.fileobj.fileno(), 16384)
                    if not data:
                        select.unregister(key.fileobj)
                        continue
                    if key.fileobj is child.stderr:
                        raise ValueError('INITIALIZER_REJECTED')
                    received.extend(data)
                    if len(received) > 16384:
                        raise ValueError('INITIALIZER_OUTPUT_LIMIT')
                    if not sent:
                        if not ready.startswith(received):
                            raise ValueError('INITIALIZER_PROTOCOL')
                        if received == ready:
                            if audit() != baseline:
                                raise ValueError('APPROVAL_REVOKED_BEFORE_PUBLICATION')
                            check()
                            if stopped() or child.poll() is not None or time.monotonic() >= deadline:
                                raise ValueError('INITIALIZER_STOPPED')
                            if os.write(child.stdin.fileno(), b'PUBLISH\n') != 8:
                                raise ValueError('PUBLICATION_OUTCOME_UNKNOWN')
                            child.stdin.close()
                            sent = True
                            received.clear()
        if child.wait(timeout=5) != 0 or not sent:
            raise ValueError('INITIALIZER_FAILED_OUTCOME_UNKNOWN')
        result = decode(bytes(received))
        if (type(result) is not dict or result.get('schema') != 's3-local-initialization/1'
                or result.get('root') != str(a.output) or result.get('runtime_pin') != a.runtime_pin
                or result.get('fee_bps') != a.fee_bps or result.get('service_started') is not False
                or result.get('c_semantic_validation_verified') is not True):
            raise ValueError('INITIALIZER_REPORT_REJECTED')
        if decode(bounded(checked_root(a.output), 'initialization.json', 16384)) != result:
            raise ValueError('INITIALIZER_REPORT_CHANGED')
        return result
    finally:
        child.stdin.close()
        try:
            # EOF closes the reviewed child's publication gate. Reap its
            # normal rejection before considering a signal; never signal a
            # group identified by an already reaped child PID.
            child.wait(timeout=.25)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        finally:
            child.wait(timeout=5)
            child.stdout.close()
            child.stderr.close()


def main(argv=None):
    try:
        a = parse(sys.argv[1:] if argv is None else argv)
        revisions = {'ceo': a.ceo_revision, 'cto': a.cto_revision}
        def audit():
            return approval_gate.inspect(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, True, a.native_decision_id, revisions)
        with stop_latch() as stopped, stage(a, audit) as (root, check, baseline):
            result = run(a, root, check, baseline, audit, stopped)
        sys.stdout.buffer.write(encode(result))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt, subprocess.TimeoutExpired):
        print(ERROR, file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
