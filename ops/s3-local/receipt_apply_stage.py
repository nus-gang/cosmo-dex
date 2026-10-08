"""Private F09 child bytes plus C validation; no child start or permit."""
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

RECEIPT_APPLY = 'bin/s3-local-receipt-apply'

@contextmanager
def stage(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, arguments, scratch, timeout=60):
    revisions = dict(revisions); arguments = list(arguments)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, _ = verify_input_set(bundle, artifacts, pin, profile, acknowledge,
                              inputs, input_name)
    descriptor = decode(base64.b64decode(decode(raw)['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(RECEIPT_APPLY)
    if not isinstance(digest, str):
        raise ValueError('RECEIPT_APPLY_NOT_IN_SRE_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), RECEIPT_APPLY, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('RECEIPT_APPLY_BYTES_CHANGED')
    with tempfile.TemporaryDirectory(prefix='receipt-apply-stage-', dir=checked_root(scratch)) as directory:
        executable = Path(directory) / 's3-local-receipt-apply'
        _write(executable, binary, 0o500); del binary
        original = executable.stat()
        validate_snapshot(raw, artifacts, arguments, scratch, timeout)
        after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                      decision_id, revisions)
        if before != after:
            raise ValueError('APPROVAL_CHANGED_DURING_RECEIPT_APPLY_STAGE')
        current = executable.lstat()
        if (current.st_dev, current.st_ino, current.st_mode, current.st_uid, current.st_nlink) != (
                original.st_dev, original.st_ino, original.st_mode, os.geteuid(), 1):
            raise ValueError('STAGED_RECEIPT_APPLY_CHANGED')
        if hashlib.sha256(bounded(checked_root(directory), executable.name, MAX_BINARY)).hexdigest() != digest:
            raise ValueError('STAGED_RECEIPT_APPLY_CHANGED')
        yield StagedWorker(executable, raw, hashlib.sha256(raw).hexdigest(), digest)
