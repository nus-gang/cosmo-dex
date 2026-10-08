"""Bounded one-shot storage fault supervisor for an authenticated staged child."""
import json
import math
import os
import selectors
import socket
import time
from ready_worker import _ready_scope, check_bytes
from storage_fault_ready import fault_arguments


def run(staged, arguments, audit, *, point, occurrence, purpose, evidence_root,
        enable=False, errno=None, operation="Seal", stop=lambda: False, timeout=5, lifetime=60):
    """Use only within storage_fault_stage.stage. No retries or evidence deletion.

    START can persist an observation and poison the store. A missing/invalid
    child result leaves outcome unknown; a valid result is not replay or DEV
    verification. The caller owns authenticated approval and runtime placement.
    """
    if operation not in ("Seal", "Apply") or (operation == "Apply" and purpose != "NORMAL"):
        raise ValueError("STORAGE_FAULT_COMMAND")
    options = fault_arguments(point, occurrence, purpose, evidence_root, enable, errno)
    arguments = list(arguments)
    if any(x.startswith('--fault-') or x == '--enable-storage-fault' for x in arguments):
        raise ValueError('DUPLICATE_FAULT_OPTIONS')
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 60:
        raise ValueError('FAULT_LIFETIME_LIMIT')
    # Exact serde_json map output, accepting both command outcomes independently
    # of injection. A successful process is not a successful Seal.
    reports = {}
    for succeeded in (False, True):
        for injected in (False, True):
            obj = dict(schema='s3-local-fault-'+operation.lower()+'-result/1', command_succeeded=succeeded,
                       injected=injected, durable_ack=False, DEV='NOT_RUN')
            reports[(json.dumps(obj, sort_keys=True, separators=(',', ':'))+'\n').encode()] = obj
    with _ready_scope(staged, options+arguments, audit, timeout, stop,
                      command='fault-'+operation.lower()+'-captured') as (child, gate, baseline, deadline):
        if audit() != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_FAULT')
        check_bytes(staged)
        if stop() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('FAULT_STOPPED_BEFORE_START')
        gate.settimeout(min(1, max(.001, deadline-time.monotonic())))
        gate.sendall(b'START\n')
        gate.shutdown(socket.SHUT_WR)
        end = time.monotonic()+lifetime
        output = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ, 'stdout')
            selector.register(child.stderr, selectors.EVENT_READ, 'stderr')
            while True:
                if stop():
                    raise ValueError('FAULT_STOPPED_OUTCOME_UNKNOWN')
                remaining = end-time.monotonic()
                if remaining <= 0:
                    raise ValueError('FAULT_TIMEOUT_OUTCOME_UNKNOWN')
                for key, _ in selector.select(min(.05, remaining)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    elif key.data == 'stderr':
                        raise ValueError('FAULT_REJECTED_OUTCOME_UNKNOWN')
                    else:
                        output.extend(data)
                        if not any(raw.startswith(output) for raw in reports):
                            raise ValueError('FAULT_REPORT_OUTCOME_UNKNOWN')
                code = child.poll()
                if code is not None and not selector.get_map():
                    result = reports.get(bytes(output))
                    if code != 0 or result is None:
                        raise ValueError('FAULT_FAILED_OUTCOME_UNKNOWN')
                    return dict(child_result=result, child_exit=0, fault_started=True,
                                approval_verified=False, reusable_permit=False,
                                replay_verified=False)
