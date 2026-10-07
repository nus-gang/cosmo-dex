"""Internal managed web lifecycle. Actual service execution belongs to L-T.

No cached PreparedWeb is accepted: every invocation prepares captured assets,
validates C inputs and rereads independent approvals immediately before bind.
The eventual managed CLI supplies authenticated approval reader scope/signals.
"""
from pathlib import Path
from fault_evidence import record
import socket
import time
import approval_gate
import reviewed_web
from web_proxy import WebProxy
from web_upstream import Upstream
from web_connection import serve_listener


def run(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions,
        inputs, input_name, arguments, scratch, origin, worker_port, *,
        max_requests, lifetime_seconds, stop, timeout=60,
        socket_factory=socket.socket, clock=time.monotonic, on_ready=None, response_loss_sha256=None, fault_evidence_root=None):
    if (type(max_requests) is not int or not 1 <= max_requests <= 10000 or
        type(lifetime_seconds) is not int or not 1 <= lifetime_seconds <= 3600 or
        type(worker_port) is not int or not 1024 <= worker_port <= 65535 or
        worker_port == 5173 or origin not in
        ('http://127.0.0.1:5173', 'http://localhost:5173') or not callable(stop)):
        raise ValueError('WEB_RUN_INPUT')
    if on_ready is not None and not callable(on_ready):
        raise ValueError('WEB_PID_CALLBACK')
    # Validate fault selection before approval IO or socket creation.
    if response_loss_sha256 is not None:
        from response_loss import BroadcastResponseLoss
        BroadcastResponseLoss(lambda *_: None, destination=('127.0.0.1', worker_port),
            body_sha256=response_loss_sha256, enable_local_demo=profile == 's3-dev-local/1',
            allow_unproven_host_space=acknowledge)
    if (response_loss_sha256 is None) != (fault_evidence_root is None):
        raise ValueError('FAULT_EVIDENCE_REQUIRED')
    if fault_evidence_root is not None:
        path = Path(fault_evidence_root)
        if not path.is_absolute() or '..' in path.parts:
            raise ValueError('FAULT_EVIDENCE_PATH')
    revisions, arguments = dict(revisions), list(arguments)

    def check_stop():
        if stop():
            raise InterruptedError('WEB_STOPPED')

    check_stop()
    before = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                   decision_id, revisions)
    prepared = reviewed_web.prepare(bundle, artifacts, pin, profile, acknowledge,
        decision_id, revisions, inputs, input_name, arguments, scratch, origin, timeout)
    proxy = WebProxy(origin=origin, worker_port=worker_port,
                     static=prepared.response_boundary)
    exchange = Upstream(clock=clock)
    if response_loss_sha256 is not None:
        exchange = BroadcastResponseLoss(exchange, destination=proxy.destination,
            body_sha256=response_loss_sha256, enable_local_demo=profile == 's3-dev-local/1',
            allow_unproven_host_space=acknowledge)
    check_stop()
    after = approval_gate.inspect(bundle, artifacts, pin, profile, acknowledge,
                                  decision_id, revisions)
    if before != after:
        raise ValueError('APPROVAL_CHANGED_BEFORE_WEB_BIND')
    check_stop()
    def serve():
        if on_ready is not None:
            on_ready()
        check_stop()
        listener = socket_factory(socket.AF_INET, socket.SOCK_STREAM)
        owned = True
        try:
            check_stop()
            # No SO_REUSEPORT/SO_REUSEADDR: occupied ports fail closed.
            listener.bind(('127.0.0.1', 5173))
            check_stop()
            listener.listen(1)
            check_stop()
            owned = False  # serve_listener owns close on every path.
            result = serve_listener(listener, proxy, exchange,
                max_requests=max_requests, lifetime_seconds=lifetime_seconds,
                stop=stop, clock=clock)
            if response_loss_sha256 is not None:
                result = dict(result, response_loss=exchange.report())
            return dict(result, capture_sha256=prepared.capture_sha256,
                        validator_sha256=prepared.validator_sha256,
                        reusable_permit=False)
        finally:
            if owned:
                listener.close()
    if response_loss_sha256 is not None:
        return record(fault_evidence_root, exchange, serve)
    return serve()
