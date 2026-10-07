"""Internal create supervision; trusted RPC/auth callers required, no retries."""
import math
import os
import selectors
import socket
import time
from bootstrap_ready import _scope
from ready_worker import check_bytes

SUCCESS = b'{"home_created":true,"approval_verified":false,"service_started":false,"durable_ack":false}\n'


def run(staged, home, evidence_root, pin, effective_profile, audit,
        stop=lambda: False, timeout=5, lifetime=60):
    """Only inside bootstrap_stage.stage with current authenticated audit.

    START may create a home. Any failure afterwards leaves creation outcome
    unknown: preserve home/evidence and never retry, remove or reseed here.
    This report describes the child result, not independent replay verification.
    """
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 60:
        raise ValueError('BOOTSTRAP_LIFETIME_LIMIT')
    with _scope(staged, home, evidence_root, pin, effective_profile, audit,
                stop, timeout) as (state, checked_audit):
        child, gate, baseline, deadline = state
        if checked_audit() != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_CREATE')
        check_bytes(staged)
        if stop() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('BOOTSTRAP_STOPPED_BEFORE_CREATE')
        gate.settimeout(min(1, max(.001, deadline-time.monotonic())))
        gate.sendall(b'START\n')
        gate.shutdown(socket.SHUT_WR)
        end = time.monotonic() + lifetime
        output = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ, 'stdout')
            selector.register(child.stderr, selectors.EVENT_READ, 'stderr')
            while True:
                if stop():
                    raise ValueError('BOOTSTRAP_STOPPED_OUTCOME_UNKNOWN')
                remaining = end-time.monotonic()
                if remaining <= 0:
                    raise ValueError('BOOTSTRAP_TIMEOUT_OUTCOME_UNKNOWN')
                for key, _ in selector.select(min(.05, remaining)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    elif key.data == 'stderr':
                        raise ValueError('BOOTSTRAP_REJECTED_OUTCOME_UNKNOWN')
                    else:
                        output.extend(data)
                        if not SUCCESS.startswith(output):
                            raise ValueError('BOOTSTRAP_REPORT_OUTCOME_UNKNOWN')
                code = child.poll()
                if code is not None and not selector.get_map():
                    if code != 0 or output != SUCCESS:
                        raise ValueError('BOOTSTRAP_FAILED_OUTCOME_UNKNOWN')
                    return {'child_reported_home_created': True, 'child_exit': 0,
                            'service_started': False, 'approval_verified': False,
                            'reusable_permit': False, 'replay_verified': False}
