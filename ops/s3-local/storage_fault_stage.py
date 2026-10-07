"""Private fault-build bytes plus C validation; no child start or permit."""
import base64
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import tempfile
import approval_gate
from bootstrap_stage import _write
from manifest import decode
from offline_check import DESCRIPTOR, MAX_BINARY, validate_snapshot
from preflight import bounded, checked_root, verify_input_set
from staged_worker import StagedWorker

FAULT = 'bin/s3-local-storage-fault'

@contextmanager
def stage(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, arguments, scratch, timeout=60):
    """Check the dedicated descriptor entry, never fall back to ordinary worker.

    arguments are existing validator/worker inputs, without fault options. The
    eventual parent owns explicit fault selection, READY and start-time audit.
    Only this temporary copy is removed; home and fault evidence are untouched.
    """
    revisions = dict(revisions)
    arguments = list(arguments)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, _ = verify_input_set(bundle, artifacts, pin, profile, acknowledge,
                              inputs, input_name)
    descriptor = decode(base64.b64decode(decode(raw)['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(FAULT)
    if not isinstance(digest, str):
        raise ValueError('FAULT_NOT_IN_SRE_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), FAULT, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('FAULT_BYTES_CHANGED')
    with tempfile.TemporaryDirectory(prefix='storage-fault-stage-', dir=checked_root(scratch)) as directory:
        executable = Path(directory) / 's3-local-storage-fault'
        _write(executable, binary, 0o500)
        del binary
        original = executable.stat()
        validate_snapshot(raw, artifacts, arguments, scratch, timeout)
        after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                      decision_id, revisions)
        if before != after:
            raise ValueError('APPROVAL_CHANGED_DURING_FAULT_STAGE')
        current = executable.lstat()
        if (current.st_dev, current.st_ino, current.st_mode, current.st_uid, current.st_nlink) != (
                original.st_dev, original.st_ino, original.st_mode, os.geteuid(), 1):
            raise ValueError('STAGED_FAULT_CHANGED')
        if hashlib.sha256(bounded(checked_root(directory), executable.name, MAX_BINARY)).hexdigest() != digest:
            raise ValueError('STAGED_FAULT_CHANGED')
        yield StagedWorker(executable, raw, hashlib.sha256(raw).hexdigest(), digest)
