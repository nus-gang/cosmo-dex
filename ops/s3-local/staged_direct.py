"""Exact direct-TX helper snapshot; internal byte boundary, never a permit."""
import base64
from contextlib import contextmanager
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import re
import tempfile
from manifest import decode
from offline_check import DESCRIPTOR, MAX_BINARY
from preflight import bounded, checked_root

HELPER = 'bin/nus-s3-local-direct'

@dataclass(frozen=True)
class StagedDirect:
    executable: Path
    executable_sha256: str
    capture_sha256: str

    def recheck(self):
        raw = bounded(checked_root(self.executable.parent), self.executable.name, MAX_BINARY)
        if hashlib.sha256(raw).hexdigest() != self.executable_sha256:
            raise ValueError('STAGED_DIRECT_BYTES_CHANGED')

@contextmanager
def stage_snapshot(capture, artifacts, scratch):
    """Caller supplies the same byte-verified capture used for worker validation.

    Does not authenticate that capture or approve execution. Keep this scope open
    through child reap; pass only its private executable to direct_child::verify.
    """
    if not isinstance(capture, bytes):
        raise ValueError('IMMUTABLE_CAPTURE_REQUIRED')
    descriptor = decode(base64.b64decode(decode(capture)['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(HELPER)
    if not isinstance(digest, str) or re.fullmatch('[0-9a-f]{64}', digest) is None:
        raise ValueError('DIRECT_NOT_IN_SRE_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), HELPER, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('DIRECT_BYTES_CHANGED')
    with tempfile.TemporaryDirectory(prefix='staged-direct-', dir=checked_root(scratch)) as directory:
        executable = Path(directory) / 'nus-s3-local-direct'
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
        staged = StagedDirect(executable, digest, hashlib.sha256(capture).hexdigest())
        staged.recheck()
        yield staged
