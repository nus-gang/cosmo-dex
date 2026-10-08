"""Ephemeral exact worker bytes. No execution, semantic validation or permit."""
import base64
from contextlib import contextmanager
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import tempfile
import approval_gate
from manifest import decode
from offline_check import DESCRIPTOR, MAX_BINARY
from preflight import bounded, checked_root, verify_input_set

WORKER = 'bin/s3-local-worker'

@dataclass(frozen=True)
class StagedWorker:
    executable: Path
    capture: bytes
    capture_sha256: str
    executable_sha256: str

@contextmanager
def stage(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, scratch):
    # Not a reusable permit: a future managed launcher must consume this scope,
    # validate semantics and recheck approval immediately before service start.
    revisions = dict(revisions)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, _ = verify_input_set(bundle, artifacts, pin, profile, acknowledge,
                              inputs, input_name)
    descriptor = decode(base64.b64decode(decode(raw)['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(WORKER)
    if not isinstance(digest, str):
        raise ValueError('WORKER_NOT_IN_SRE_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), WORKER, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('WORKER_BYTES_CHANGED')
    with tempfile.TemporaryDirectory(prefix='staged-worker-', dir=checked_root(scratch)) as directory:
        executable = Path(directory) / 's3-local-worker'
        fd = os.open(executable, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            with os.fdopen(fd, 'wb', closefd=False) as stream:
                stream.write(binary)
                stream.flush()
                os.fsync(fd)
            os.fchmod(fd, 0o500)
        finally:
            os.close(fd)
        del binary
        after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                      decision_id, revisions)
        if before != after:
            raise ValueError('APPROVAL_CHANGED_DURING_STAGING')
        # Directory is owned by this context. Never deletes home, keys or source.
        yield StagedWorker(executable, raw, hashlib.sha256(raw).hexdigest(), digest)


@contextmanager
def validated_stage(bundle, artifacts, pin, profile, acknowledge, decision_id,
                    revisions, inputs, input_name, arguments, scratch, timeout=60):
    """Stage and semantically check the same snapshot; never start a service.

    A managed launcher still needs a start-time gate. The yielded scope is not
    a reusable permit and is valid only while its private directory exists.
    """
    from offline_check import validate_snapshot
    revisions = dict(revisions)
    arguments = list(arguments)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    with stage(bundle, artifacts, pin, profile, acknowledge, decision_id,
               revisions, inputs, input_name, scratch) as staged:
        validate_snapshot(staged.capture, artifacts, arguments, scratch, timeout)
        after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                      decision_id, revisions)
        if before != after:
            raise ValueError('APPROVAL_CHANGED_DURING_VALIDATION')
        binary = bounded(checked_root(staged.executable.parent),
                         staged.executable.name, MAX_BINARY)
        if hashlib.sha256(binary).hexdigest() != staged.executable_sha256:
            raise ValueError('STAGED_WORKER_BYTES_CHANGED')
        del binary
        yield staged


@contextmanager
def checked_ready(bundle, artifacts, pin, profile, acknowledge, decision_id,
                  revisions, inputs, input_name, arguments, scratch, timeout=60):
    """Validation + READY + fresh authenticated audit; service start remains off."""
    from direct_worker import ready_with_direct
    revisions = dict(revisions)
    arguments = list(arguments)
    def audit():
        return approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                     decision_id, revisions)
    baseline = audit()
    def same_candidate():
        current = audit()
        if current != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_READY')
        return current
    with validated_stage(bundle, artifacts, pin, profile, acknowledge, decision_id,
                         revisions, inputs, input_name, arguments, scratch, timeout) as staged:
        with ready_with_direct(staged, arguments, artifacts, scratch, same_candidate, timeout) as report:
            yield report


def checked_run(bundle, artifacts, pin, profile, acknowledge, decision_id,
                revisions, inputs, input_name, arguments, scratch, stop,
                timeout=60, lifetime=300, *, on_spawn=None):
    """L-T managed foreground worker path; never invoke during L-R checks."""
    from direct_worker import run_with_direct
    revisions = dict(revisions)
    arguments = list(arguments)
    def audit():
        return approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                     decision_id, revisions)
    baseline = audit()
    def same_candidate():
        current = audit()
        if current != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_START')
        return current
    with validated_stage(bundle, artifacts, pin, profile, acknowledge, decision_id,
                         revisions, inputs, input_name, arguments, scratch, timeout) as staged:
        return run_with_direct(staged, arguments, artifacts, scratch, same_candidate, stop, timeout, lifetime,
                           on_spawn=on_spawn)
