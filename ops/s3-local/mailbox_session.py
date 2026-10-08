"""Bind one managed worker session to its private PID evidence.

No service entry point. The mailbox is created before command registration by
its owner and retained on every exit, including incomplete/ambiguous starts.
"""
from contextlib import contextmanager
import copy
from pathlib import Path

import managed_session
import release_inventory
from offline_cli import parse
from runtime_config import worker_config
from worker_inventory import worker_endpoints

ERROR = 'MAILBOX_SESSION_REJECTED'


@contextmanager
def session(python, candidate, argv, *, mailbox, raw, artifacts, arguments,
            scratch, validator_sha256, **kwargs):
    """Collect once after confirmed stop, then observe only the listed resources.

    Same-uid mailbox provenance and trusted capture inputs are prerequisites.
    This cannot prove all descendants were listed or grant runtime approval.
    """
    try:
        argv = tuple(argv)
        parsed, _ = parse(list(argv), reviewed=True, managed=True)
        if parsed.pid_mailbox != Path(mailbox.root) or mailbox.used:
            raise ValueError()
        endpoints = worker_endpoints(worker_config(python, candidate, argv,
            fee_bps=kwargs['fee_bps']), python, candidate, fee_bps=kwargs['fee_bps'])
        # Freeze capture and invocation before any managed control request.
        artifacts = copy.deepcopy(artifacts)
        arguments = tuple(arguments)
        if not isinstance(raw, bytes) or not raw:
            raise ValueError()
    except Exception:
        raise ValueError(ERROR) from None

    probes = None
    consumed = False
    evidence = None
    def invoke(name):
        nonlocal probes, consumed
        if probes is None:
            if consumed:
                raise ValueError(ERROR)
            consumed = True
            pids = mailbox.collect()
            probes = release_inventory.probes(pids, endpoints, raw, artifacts,
                arguments, scratch, validator_sha256)
            evidence['pid_handoff_collected'] = True
            evidence['listed_pids'] = list(pids)
        return probes[name]()

    with managed_session._release_session(python, candidate, argv,
            process_probe=lambda: invoke('process_probe'),
            port_probe=lambda: invoke('port_probe'),
            writer_probe=lambda: invoke('writer_probe'), **kwargs) as evidence:
        evidence['pid_handoff_collected'] = False
        evidence['inventory_complete_verified'] = False
        evidence['pid_evidence_retained'] = True
        yield evidence
