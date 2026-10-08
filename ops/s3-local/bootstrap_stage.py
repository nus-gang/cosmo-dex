"""Private bootstrap inputs; no create child execution or reusable permission."""
import base64
from contextlib import contextmanager
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import tempfile
import approval_gate
from bootstrap_check import CHECKER
from manifest import decode
from offline_check import DESCRIPTOR, MAX_BINARY, _validate_snapshot
from preflight import bounded, checked_root, verify_input_set

CREATOR = 'bin/s3-local-bootstrap-create'
MAX_RPC = 16 * 1024 * 1024  # C evidence::limit(application/json)

@dataclass(frozen=True)
class StagedBootstrap:
    executable: Path
    executable_sha256: str
    capture: bytes
    capture_sha256: str
    rpc_file: Path
    rpc_sha256: str


def _write(path, raw, mode):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, 'wb', closefd=False) as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(fd)
        os.fchmod(fd, mode)
    finally:
        os.close(fd)


@contextmanager
def stage(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, effective_profile, rpc, scratch, timeout=60):
    """Caller supplies bounded raw RPC; child later preserves it as evidence.

    The caller must acquire RPC through the trusted Chain reader and re-audit
    after READY before START. This scope neither validates RPC semantics nor
    authorizes creation. Temporary copies are removed; evidence/home are untouched.
    """
    effective_profile = Path(effective_profile)
    if not effective_profile.is_absolute() or '..' in effective_profile.parts:
        raise ValueError('INPUT_PATH')
    if not isinstance(rpc, bytes) or not 0 < len(rpc) <= MAX_RPC:
        raise ValueError('RPC_BYTES')
    revisions = dict(revisions)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, _ = verify_input_set(bundle, artifacts, pin, profile, acknowledge,
                              inputs, input_name)
    descriptor = decode(base64.b64decode(decode(raw)['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(CREATOR)
    if not isinstance(digest, str):
        raise ValueError('CREATOR_NOT_IN_SRE_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), CREATOR, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('CREATOR_BYTES_CHANGED')
    with tempfile.TemporaryDirectory(prefix='bootstrap-stage-', dir=checked_root(scratch)) as directory:
        executable = Path(directory) / 's3-local-bootstrap-create'
        rpc_file = Path(directory) / 'rpc.json'
        _write(executable, binary, 0o500)
        _write(rpc_file, rpc, 0o600)
        arguments = ['--runtime-pin', pin, '--local-demo-profile',
                     str(effective_profile), '--acknowledge-unproven-space']
        _validate_snapshot(raw, artifacts, arguments, scratch, timeout, CHECKER)
        after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                      decision_id, revisions)
        if before != after:
            raise ValueError('APPROVAL_CHANGED_DURING_BOOTSTRAP_STAGE')
        for path, expected, cap in [(executable, digest, MAX_BINARY),
                (rpc_file, hashlib.sha256(rpc).hexdigest(), MAX_RPC)]:
            if hashlib.sha256(bounded(checked_root(directory), path.name, cap)).hexdigest() != expected:
                raise ValueError('BOOTSTRAP_STAGED_BYTES_CHANGED')
        yield StagedBootstrap(executable, digest, raw, hashlib.sha256(raw).hexdigest(),
                              rpc_file, hashlib.sha256(rpc).hexdigest())
