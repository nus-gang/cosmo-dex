#!/usr/bin/env python3
"""Capture-to-validator wiring only. Never grants runtime approval."""
import base64
import hashlib
import os
from pathlib import Path
import tempfile
from manifest import decode
from preflight import bounded, checked_root, verify_input_set
from process_check import validate_captured

VALIDATOR = 'bin/s3-local-preflight'
DESCRIPTOR = 'chain/local-demo/components/sre.json'
MAX_BINARY = 512 * 1024 * 1024


def check(bundle, artifacts, pin, profile, acknowledge, inputs, input_name,
          arguments, scratch, timeout=60):
    raw, byte_report = verify_input_set(bundle, artifacts, pin, profile,
                                       acknowledge, inputs, input_name)
    digest, semantic = validate_snapshot(raw, artifacts, arguments, scratch, timeout)
    return {'byte_preflight': byte_report, 'validator_sha256': digest,
            'semantic_preflight': semantic, 'approval_verified': False,
            'services_started': False, 'DEV': 'NOT_RUN'}


def validate_snapshot(raw, artifacts, arguments, scratch, timeout=60):
    """Validate an already byte-verified capture without reopening input paths.

    Internal composition boundary: caller owns byte/provenance verification.
    This does not independently approve the capture or return a launch permit.
    """
    return _validate_snapshot(raw, artifacts, arguments, scratch, timeout, VALIDATOR)


def _validate_snapshot(raw, artifacts, arguments, scratch, timeout, artifact_name):
    # Internal shared transport, called only by fixed-purpose wrappers.
    # Read the descriptor from the already verified capture, never reread the
    # bundle path after validation. This is byte provenance, not approval.
    capture = decode(raw)
    descriptor = decode(base64.b64decode(capture['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(artifact_name)
    if not isinstance(digest, str):
        raise ValueError('VALIDATOR_NOT_IN_SRE_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), artifact_name, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('VALIDATOR_BYTES_CHANGED')
    scratch = checked_root(scratch)
    # Execute the bytes just hashed, rather than the mutable original path.
    # Private scratch assumes the same trusted local uid as the reviewed home;
    # this is not isolation from a malicious same-uid process or interpreter.
    with tempfile.TemporaryDirectory(prefix='offline-validator-', dir=scratch) as directory:
        executable = Path(directory) / 's3-local-preflight'
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
        semantic = validate_captured(str(executable), arguments, raw, timeout)
    return digest, semantic
