"""Current-run authenticated web orchestration; invocation belongs to L-T.

No registration writes, automatic start, token files, or reusable start permit.
The web command independently rechecks approval/capture before binding.
"""
from contextlib import contextmanager
import hashlib
import approval_gate
import preflight
import reviewed_web
import web_session
from private_reader import broker_at
from runtime_client import RuntimeClient
from web_cli import parse_web
from web_pid_mailbox import WebMailbox

ERROR = 'AUTHENTICATED_WEB_SESSION_REJECTED'


@contextmanager
def session(python, candidate, argv, workspace_id, *, fee_bps, mailbox,
            stop=lambda: False):
    if (type(mailbox) is not WebMailbox or mailbox.used or
            getattr(mailbox, '_authenticated_attempted', False)):
        raise ValueError(ERROR)
    mailbox._authenticated_attempted = True
    try:
        argv = tuple(argv)
        a, arguments, endpoint, origin, _, _, decision, revisions = parse_web(
            ['serve-web-reviewed', *argv])
        if type(fee_bps) is not int or fee_bps not in (0, 25) or a.pid_mailbox != mailbox.root:
            raise ValueError()
        client = RuntimeClient.from_environment(workspace_id, 's3-web-fee'+str(fee_bps))

        def inspect():
            if stop():
                raise ValueError(ERROR)
            return approval_gate.inspect(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, a.acknowledge_unproven_space, decision, revisions)

        def capture():
            return preflight.verify_input_set(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, a.acknowledge_unproven_space,
                a.input_set.parent, a.input_set.name)[0]

        before = inspect()
        raw = capture()
        prepared = reviewed_web.prepare(a.bundle, a.artifacts, a.runtime_pin,
            a.local_demo_profile, a.acknowledge_unproven_space, decision, revisions,
            a.input_set.parent, a.input_set.name, arguments, a.scratch, origin)
        if prepared.capture_sha256 != hashlib.sha256(raw).hexdigest():
            raise ValueError()

        def audit():
            if stop() or capture() != raw or inspect() != before or stop():
                raise ValueError(ERROR)

        audit()
        with web_session.session(python, candidate, argv, fee_bps=fee_bps,
                mailbox=mailbox, broker_root=endpoint.parent, broker=broker_at,
                client=client, audit=audit) as evidence:
            yield evidence
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception:
        raise ValueError(ERROR) from None
