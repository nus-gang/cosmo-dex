"""Reviewed one-shot Snapshot transport; not a create permit or chain proof."""
import base64
import hashlib
from pathlib import Path
import tempfile
import approval_gate
from bootstrap_check import CHECKER
from bootstrap_stage import _write
from fetch_process import FetchFailure, fetch_captured
from manifest import decode
from offline_check import DESCRIPTOR, MAX_BINARY, _validate_snapshot
from preflight import bounded, checked_root, verify_input_set

FETCHER = 'bin/s3-local-bootstrap-fetch'


def fetch(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, effective_profile, address, scratch, timeout=10,
          stopped=lambda: False):
    """Authenticate, capture, check, re-audit, fetch once, re-audit.

    Returned RPC bytes must still be durably preserved and checked by C before
    separately authorized create. Post-fetch rejection retains received bytes
    in FetchFailure.partial_raw, never as a successful Snapshot. No retry.
    """
    effective_profile = Path(effective_profile)
    if not effective_profile.is_absolute() or '..' in effective_profile.parts:
        raise ValueError('INPUT_PATH')
    revisions = dict(revisions)
    if stopped():
        raise FetchFailure('FETCH_STOPPED')
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, _ = verify_input_set(bundle, artifacts, pin, profile, acknowledge,
                             inputs, input_name)
    descriptor = decode(base64.b64decode(decode(raw)['files'][DESCRIPTOR], validate=True))
    inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    digest = inventory.get(FETCHER)
    if not isinstance(digest, str):
        raise ValueError('FETCHER_NOT_IN_SRE_DESCRIPTOR')
    binary = bounded(checked_root(artifacts), FETCHER, MAX_BINARY)
    if not binary or hashlib.sha256(binary).hexdigest() != digest:
        raise ValueError('FETCHER_BYTES_CHANGED')
    with tempfile.TemporaryDirectory(prefix='bootstrap-fetch-', dir=checked_root(scratch)) as directory:
        executable = Path(directory) / 's3-local-bootstrap-fetch'
        _write(executable, binary, 0o500)
        arguments = ['--runtime-pin', pin, '--local-demo-profile',
                     str(effective_profile), '--acknowledge-unproven-space']
        _validate_snapshot(raw, artifacts, arguments, scratch, timeout, CHECKER)

        def recheck():
            audit = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                          decision_id, revisions)
            if audit != before:
                raise ValueError('APPROVAL_CHANGED_DURING_FETCH')
            if hashlib.sha256(bounded(checked_root(directory), executable.name, MAX_BINARY)).hexdigest() != digest:
                raise ValueError('FETCHER_STAGED_BYTES_CHANGED')
            if stopped():
                raise ValueError('FETCH_STOPPED')

        recheck()
        received = fetch_captured(executable, address, pin, effective_profile,
                                  acknowledge, raw, timeout, stopped)
        try:
            recheck()
        except Exception:
            raise FetchFailure('FETCH_POSTCHECK_REJECTED', received) from None
        return received
