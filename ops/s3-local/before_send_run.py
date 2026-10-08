"""Bounded one-shot F05 supervisor for an authenticated staged child."""
import math
import os
import selectors
import socket
import time

from before_send_ready import before_send_arguments
from ready_worker import _ready_scope, check_bytes


def run(staged, arguments, audit, *, tx_hash, evidence_root, enable=False,
        stop=lambda: False, timeout=5, lifetime=60):
    """Start the isolated F05 child once; never interpret exit 86 as success.

    The durable boundary report and replay must be inspected independently.
    Any post-START ambiguity remains UNKNOWN and the caller must not retry.
    """
    options = before_send_arguments(tx_hash, evidence_root, enable)
    arguments = list(arguments)
    if any(not isinstance(arg, str) or arg.startswith(('--fault-', '--enable-f14',
           '--enable-f05', '--tx-hash', '--enable-storage')) or arg == '--worker-inputs'
           for arg in arguments):
        raise ValueError('DUPLICATE_F05_OPTIONS')
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 60:
        raise ValueError('F05_LIFETIME_LIMIT')
    with _ready_scope(staged, options + arguments, audit, timeout, stop,
                      command='f05-crash-captured') as (child, gate, baseline, deadline):
        if audit() != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_F05')
        check_bytes(staged)
        if stop() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('F05_STOPPED_BEFORE_START')
        gate.settimeout(min(1, max(.001, deadline - time.monotonic())))
        gate.sendall(b'START\n')
        gate.shutdown(socket.SHUT_WR)
        end = time.monotonic() + lifetime
        output = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ, 'stdout')
            selector.register(child.stderr, selectors.EVENT_READ, 'stderr')
            while True:
                if stop():
                    raise ValueError('F05_STOPPED_OUTCOME_UNKNOWN')
                remaining = end - time.monotonic()
                if remaining <= 0:
                    raise ValueError('F05_TIMEOUT_OUTCOME_UNKNOWN')
                for key, _ in selector.select(min(.05, remaining)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    else:
                        output.extend(data)
                        raise ValueError('F05_OUTPUT_OUTCOME_UNKNOWN')
                code = child.poll()
                if code is not None and not selector.get_map():
                    if code != 86 or output:
                        raise ValueError('F05_FAILED_OUTCOME_UNKNOWN')
                    return {
                        'child_result': None,
                        'child_exit': 86,
                        'fault_started': True,
                        'outcome': 'UNKNOWN',
                        'transport_called_verified': False,
                        'crash_verified': False,
                        'approval_verified': False,
                        'reusable_permit': False,
                        'replay_verified': False,
                        'F05_verified': False,
                    }
