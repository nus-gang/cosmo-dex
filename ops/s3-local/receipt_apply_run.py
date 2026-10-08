"""Bounded one-shot F09 supervisor for an authenticated staged child."""
import math
import os
import selectors
import socket
import time
from receipt_apply_ready import receipt_apply_arguments, _arguments
from ready_worker import _ready_scope, check_bytes

def run(staged, arguments, audit, *, batch_id, evidence_root, enable=False,
        stop=lambda: False, timeout=5, lifetime=60):
    options = receipt_apply_arguments(batch_id, evidence_root, enable)
    arguments = _arguments(arguments)
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 60:
        raise ValueError('F09_LIFETIME_LIMIT')
    with _ready_scope(staged, options + arguments, audit, timeout, stop,
                      command='f09-crash-captured') as (child, gate, baseline, deadline):
        if audit() != baseline: raise ValueError('APPROVAL_CHANGED_BEFORE_F09')
        check_bytes(staged)
        if stop() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('F09_STOPPED_BEFORE_START')
        gate.settimeout(min(1, max(.001, deadline - time.monotonic())))
        gate.sendall(b'START\n'); gate.shutdown(socket.SHUT_WR)
        end = time.monotonic() + lifetime; output = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            selector.register(child.stderr, selectors.EVENT_READ)
            while True:
                if stop(): raise ValueError('F09_STOPPED_OUTCOME_UNKNOWN')
                remaining = end - time.monotonic()
                if remaining <= 0: raise ValueError('F09_TIMEOUT_OUTCOME_UNKNOWN')
                for key, _ in selector.select(min(.05, remaining)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data: selector.unregister(key.fileobj)
                    else: output.extend(data); raise ValueError('F09_OUTPUT_OUTCOME_UNKNOWN')
                code = child.poll()
                if code is not None and not selector.get_map():
                    if code != 86 or output: raise ValueError('F09_FAILED_OUTCOME_UNKNOWN')
                    return {'child_result': None, 'child_exit': 86, 'fault_started': True,
                        'outcome': 'UNKNOWN', 'apply_called_verified': False,
                        'crash_verified': False, 'receipt_durability_verified': False,
                        'approval_verified': False, 'reusable_permit': False,
                        'replay_verified': False, 'durable_ack': False,
                        'DEV': 'NOT_RUN', 'F09_verified': False}
