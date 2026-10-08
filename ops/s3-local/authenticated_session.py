"""L-T composition of current-run authentication and exact prepared command.

No CLI, registration write, or automatic invocation. The caller must already
have the exact packet registered through an authorized Paperclip path. This
context owns start/stop; its body performs the separately authorized L-T work.
"""
from contextlib import contextmanager

import approval_gate
import offline_check
import preflight
from native_review import _uuid
from offline_cli import parse
from private_reader import broker_at
from runtime_client import RuntimeClient

ERROR = 'AUTHENTICATED_SESSION_REJECTED'


@contextmanager
def session(prepared, workspace_id, *, stop=lambda: False):
    # Internal PreparedSession state is trusted; no serialized permits/adapters.
    if getattr(prepared, '_authenticated_attempted', False) or prepared._used:
        raise ValueError(ERROR)
    prepared._authenticated_attempted = True
    try:
        a, arguments = parse(prepared._argv, reviewed=True, managed=True)
        decision = _uuid(a.native_decision_id)
        revisions = {role: _uuid(getattr(a, role + '_revision'))
                     for role in ('ceo', 'cto')}
        client = RuntimeClient.from_environment(workspace_id,
                    's3-worker-fee' + str(prepared._fee))

        def inspect():
            return approval_gate.inspect(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, a.acknowledge_unproven_space,
                decision, revisions)

        if stop():
            raise ValueError(ERROR)
        before = inspect()
        raw, _ = preflight.verify_input_set(a.bundle, a.artifacts, a.runtime_pin,
            a.local_demo_profile, a.acknowledge_unproven_space,
            a.input_set.parent, a.input_set.name)
        digest, _ = offline_check.validate_snapshot(raw, a.artifacts, arguments, a.scratch)

        def audit():
            # Worker captures independently. Refuse a changed path before the
            # control request; the worker retains its own READY/START audits.
            if stop():
                raise ValueError(ERROR)
            current, _ = preflight.verify_input_set(a.bundle, a.artifacts,
                a.runtime_pin, a.local_demo_profile, a.acknowledge_unproven_space,
                a.input_set.parent, a.input_set.name)
            if current != raw or inspect() != before or stop():
                raise ValueError(ERROR)

        audit()
        with prepared.session(client=client, broker_root=a.approval_socket.parent,
                broker=broker_at, audit=audit, raw=raw, artifacts=a.artifacts,
                arguments=arguments, scratch=a.scratch,
                validator_sha256=digest) as evidence:
            yield evidence
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception:
        raise ValueError(ERROR) from None
