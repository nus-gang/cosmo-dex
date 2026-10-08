"""Bounded one-shot storage crash supervisor for an authenticated staged child."""
import json
import math
import os
import selectors
import socket
import time
from ready_worker import _ready_scope, check_bytes
from storage_crash_ready import crash_arguments


def run(staged, arguments, audit, *, point, occurrence, purpose, evidence_root,
        enable=False, fault_command="Seal", stop=lambda: False, timeout=5, lifetime=60):
    """Use only within storage_crash_stage.stage. No retries or evidence deletion.

    START can persist an observation and poison the store. A missing/invalid
    child result leaves outcome unknown; a valid result is not replay or DEV
    verification. The caller owns authenticated approval and runtime placement.
    """
    options = crash_arguments(point, occurrence, purpose, evidence_root, enable, fault_command)
    arguments = list(arguments)
    if any(not isinstance(x, str) or x.startswith('--fault-') or x.startswith('--enable-storage') or x == '--worker-inputs' for x in arguments):
        raise ValueError('DUPLICATE_CRASH_OPTIONS')
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 60:
        raise ValueError('CRASH_LIFETIME_LIMIT')
    # exit86 is only an observed process result; the reserved report and replay
    # must be inspected separately. It cannot prove which hook caused exit.
    reports = {}
    for succeeded in (False, True):
        obj = dict(schema='s3-local-crash-'+fault_command.lower()+'-result/1', command_succeeded=succeeded,
                   crash_reached=False, crash_verified=False, durable_ack=False, DEV='NOT_RUN')
        reports[(json.dumps(obj, sort_keys=True, separators=(',', ':'))+'\n').encode()] = obj
    with _ready_scope(staged, options+arguments, audit, timeout, stop,
                      command='crash-'+fault_command.lower()+'-captured') as (child, gate, baseline, deadline):
        if audit() != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_FAULT')
        check_bytes(staged)
        if stop() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('CRASH_STOPPED_BEFORE_START')
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
                    raise ValueError('CRASH_STOPPED_OUTCOME_UNKNOWN')
                remaining = end-time.monotonic()
                if remaining <= 0:
                    raise ValueError('CRASH_TIMEOUT_OUTCOME_UNKNOWN')
                for key, _ in selector.select(min(.05, remaining)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    elif key.data == 'stderr':
                        raise ValueError('CRASH_REJECTED_OUTCOME_UNKNOWN')
                    else:
                        output.extend(data)
                        if not any(raw.startswith(output) for raw in reports):
                            raise ValueError('CRASH_REPORT_OUTCOME_UNKNOWN')
                code = child.poll()
                if code is not None and not selector.get_map():
                    result = reports.get(bytes(output))
                    if code == 86 and not output:
                        return dict(child_result=None, child_exit=86, crash_started=True,
                                    outcome='UNKNOWN', crash_verified=False,
                                    approval_verified=False, reusable_permit=False,
                                    replay_verified=False)
                    if code != 0 or result is None:
                        raise ValueError('CRASH_FAILED_OUTCOME_UNKNOWN')
                    return dict(child_result=result, child_exit=0, crash_started=True,
                                outcome='RECORDED_NOT_REACHED', crash_verified=False,
                                approval_verified=False, reusable_permit=False,
                                replay_verified=False)
