"""Reviewed offline validation. Never launches services or returns a permit."""
import approval_gate
import offline_check


def check(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
          inputs, input_name, arguments, scratch, timeout=60):
    # Pin caller-owned containers across the two authenticated audits. API
    # credentials remain in the parent; offline_check isolates the child env.
    revisions = dict(revisions)
    arguments = list(arguments)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    offline = offline_check.check(bundle, artifacts, pin, profile, acknowledge,
                                  inputs, input_name, arguments, scratch, timeout)
    after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                  decision_id, revisions)
    if before != after:
        raise ValueError('APPROVAL_CHANGED_DURING_PREFLIGHT')
    return {'format': 's3-local-reviewed-offline-check/1',
            'approval_audit': after, 'offline_check': offline,
            'approval_verified': False, 'reusable_permit': False,
            'services_started': False, 'DEV': 'NOT_RUN'}
