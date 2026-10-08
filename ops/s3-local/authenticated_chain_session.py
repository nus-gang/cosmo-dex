"""Current-run Chain orchestration for L-T; no registration or automatic start.

B preflight runs against captured private bytes before the managed session.
The managed command independently repeats preflight and READY/START audits.
"""
from contextlib import contextmanager
import hashlib
import approval_gate
import chain_preflight
import chain_session
import chain_stage
import preflight
from chain_cli import parse
from pid_mailbox import Mailbox
from private_reader import broker_at
from runtime_client import RuntimeClient

ERROR = 'AUTHENTICATED_CHAIN_SESSION_REJECTED'


@contextmanager
def session(python, candidate, argv, workspace_id, *, fee_bps, validator_index,
            mailbox, stop=lambda: False):
    if (type(mailbox) is not Mailbox or mailbox.used or
            getattr(mailbox, '_authenticated_attempted', False)):
        raise ValueError(ERROR)
    mailbox._authenticated_attempted = True
    try:
        argv = tuple(argv)
        a = parse(['serve-chain-reviewed', *argv])
        if (type(fee_bps) is not int or fee_bps not in (0, 25) or
                type(validator_index) is not int or validator_index not in range(4) or
                a.pid_mailbox != mailbox.root):
            raise ValueError()
        client = RuntimeClient.from_environment(workspace_id,
            f's3-chain-fee{fee_bps}-v{validator_index}')

        def inspect():
            if stop():
                raise ValueError(ERROR)
            return approval_gate.inspect(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, a.acknowledge_unproven_space, a.decision, a.revisions)

        def capture():
            return preflight.verify_input_set(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, a.acknowledge_unproven_space,
                a.input_set.parent, a.input_set.name)[0]

        def profile():
            return preflight.bounded(preflight.checked_root(a.effective_profile.parent),
                                     a.effective_profile.name, 1024 * 1024)

        before, raw, effective = inspect(), capture(), profile()
        with chain_stage.stage(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, a.acknowledge_unproven_space, a.decision,
                a.revisions, a.input_set.parent, a.input_set.name,
                a.effective_profile, a.scratch) as staged:
            if (staged.capture_sha256 != hashlib.sha256(raw).hexdigest() or
                    staged.profile_sha256 != hashlib.sha256(effective).hexdigest()):
                raise ValueError()
            result = chain_preflight.check(staged, a.runtime_pin, str(a.home),
                a.rpc, a.p2p, a.peers, stopped=stop)
            if result != dict(b_preflight=True, approval_verified=False,
                              service_started=False, durable_ack=False):
                raise ValueError()

            def audit():
                if (stop() or capture() != raw or profile() != effective or
                        inspect() != before or stop()):
                    raise ValueError(ERROR)
                staged.verify()

            audit()
            with chain_session.session(python, candidate, argv, fee_bps=fee_bps,
                    validator_index=validator_index, mailbox=mailbox,
                    broker_root=a.approval_socket.parent, broker=broker_at,
                    client=client, audit=audit) as evidence:
                yield evidence
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception:
        raise ValueError(ERROR) from None
