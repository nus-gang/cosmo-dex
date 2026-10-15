"""Reviewed pre-create input check; no home, key, RPC or service creation."""
from pathlib import Path
import approval_gate
from offline_check import _validate_snapshot
from preflight import verify_input_set

CHECKER = 'bin/s3-local-bootstrap-check'


def check(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, effective_profile, scratch, timeout=60):
    effective_profile = Path(effective_profile)
    if not effective_profile.is_absolute() or '..' in effective_profile.parts:
        raise ValueError('INPUT_PATH')
    revisions = dict(revisions)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, byte_report = verify_input_set(bundle, artifacts, pin, profile,
                                       acknowledge, inputs, input_name)
    # No arbitrary child options and no existing-home validator fallback.
    arguments = ['--runtime-pin', pin, '--local-demo-profile',
                 str(effective_profile), '--acknowledge-unproven-space']
    digest, semantic = _validate_snapshot(raw, artifacts, arguments, scratch,
                                          timeout, CHECKER)
    after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                  decision_id, revisions)
    if before != after:
        raise ValueError('APPROVAL_CHANGED_DURING_PREFLIGHT')
    return {'format': 's3-local-bootstrap-check/1', 'approval_audit': after,
            'byte_preflight': byte_report, 'checker_sha256': digest,
            'semantic_preflight': semantic, 'approval_verified': False,
            'reusable_permit': False, 'home_created': False,
            'services_started': False, 'DEV': 'NOT_RUN'}
