"""Captured Chain launch inputs. No child execution or reusable approval."""
import base64
from contextlib import contextmanager
from dataclasses import dataclass
import hashlib
from pathlib import Path
import tempfile
import approval_gate
from bootstrap_stage import _write
from manifest import decode
from offline_check import MAX_BINARY
from preflight import bounded, checked_root, verify_input_set

CHAIN = 'bin/nus-s3-local-chain'
DESCRIPTOR = 'chain/local-demo/components/chain.json'

@dataclass(frozen=True)
class StagedChain:
    executable: Path
    input_set: Path
    effective_profile: Path
    executable_sha256: str
    capture_sha256: str
    profile_sha256: str

    def verify(self):
        for path, digest, limit in (
            (self.executable, self.executable_sha256, MAX_BINARY),
            (self.input_set, self.capture_sha256, 48 * 1024 * 1024),
            (self.effective_profile, self.profile_sha256, 1024 * 1024),
        ):
            raw = bounded(checked_root(path.parent), path.name, limit)
            if hashlib.sha256(raw).hexdigest() != digest:
                raise ValueError('CHAIN_STAGED_BYTES_CHANGED')

@contextmanager
def stage(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, effective_profile, scratch):
    """Caller must run B preflight and fresh READY/START audits in this scope.

    The profile is copied, not declared semantically valid. B validates it
    against the captured manifest/guard/genesis before creating any node.
    Private copies assume a trusted local UID; they are not a same-UID sandbox.
    """
    effective_profile = Path(effective_profile)
    profile_raw = bounded(checked_root(effective_profile.parent),
                          effective_profile.name, 1024 * 1024)
    if not profile_raw:
        raise ValueError('EMPTY_PROFILE')
    revisions = dict(revisions)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, _ = verify_input_set(bundle, artifacts, pin, profile, acknowledge,
                              inputs, input_name)
    descriptor = decode(base64.b64decode(decode(raw)['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(CHAIN)
    if not isinstance(digest, str):
        raise ValueError('CHAIN_NOT_IN_CHAIN_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), CHAIN, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('CHAIN_BYTES_CHANGED')
    with tempfile.TemporaryDirectory(prefix='chain-stage-', dir=checked_root(scratch)) as directory:
        root = Path(directory)
        executable, capture, effective = (root/'nus-s3-local-chain', root/'input.json', root/'profile.json')
        _write(executable, binary, 0o500)
        _write(capture, raw, 0o600)
        _write(effective, profile_raw, 0o600)
        staged = StagedChain(executable, capture, effective, digest,
                             hashlib.sha256(raw).hexdigest(), hashlib.sha256(profile_raw).hexdigest())
        after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                      decision_id, revisions)
        if before != after:
            raise ValueError('APPROVAL_CHANGED_DURING_CHAIN_STAGE')
        staged.verify()
        yield staged
