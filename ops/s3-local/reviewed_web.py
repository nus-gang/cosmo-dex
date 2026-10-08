"""Offline web preparation; no listener or reusable service-start permit."""
import base64
from dataclasses import dataclass
import approval_gate
import offline_check
from captured_web import capture_assets
from manifest import decode
from preflight import verify_input_set


@dataclass(frozen=True)
class PreparedWeb:
    response_boundary: object
    capture_sha256: str
    validator_sha256: str


def prepare(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
            inputs, input_name, arguments, scratch, origin, timeout=60):
    revisions, arguments = dict(revisions), list(arguments)
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    raw, _ = verify_input_set(bundle, artifacts, pin, profile, acknowledge,
                              inputs, input_name)
    assets = capture_assets(raw, artifacts)
    digest, _ = offline_check.validate_snapshot(raw, artifacts, arguments,
                                                 scratch, timeout)
    # Decode only the exact guard consumed by C. No separately supplied Context
    # or reread of a mutable input path is accepted here.
    guard = decode(base64.b64decode(decode(raw)['guard'], validate=True))
    if type(guard.get('context')) is not dict:
        raise ValueError('WEB_CONTEXT_REQUIRED')
    boundary = assets.response_boundary(origin, guard['context'])
    after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                  decision_id, revisions)
    if before != after:
        raise ValueError('APPROVAL_CHANGED_DURING_WEB_PREPARATION')
    # This object has no bind/start method. Managed start still needs a fresh
    # authenticated audit and a bounded listener lifecycle.
    return PreparedWeb(boundary, assets.capture_sha256, digest)
