"""Bounded one-shot F14 Prepare supervisor for an authenticated staged child."""
import json
import math
import os
import selectors
import socket
import time
from ready_worker import _ready_scope, check_bytes
from correction_ready import correction_arguments


def decode_report(raw, occurrence):
    try:
        obj = json.loads(raw)
        if raw != (json.dumps(obj, sort_keys=True, separators=(',', ':'))+'\n').encode():
            raise ValueError()
        if set(obj) != {'schema','command_succeeded','injected','prepare_visits',
                        'replay_visits','F14_verified','durable_ack','DEV'} or \
           obj['schema'] != 's3-local-f14-apply-result/1' or \
           obj['F14_verified'] is not False or obj['durable_ack'] is not False or obj['DEV'] != 'NOT_RUN':
            raise ValueError()
        if any(type(obj[k]) is not bool for k in ('command_succeeded','injected')) or \
           any(type(obj[k]) is not int or obj[k] < 0 for k in ('prepare_visits','replay_visits')):
            raise ValueError()
        n, r = obj['prepare_visits'], obj['replay_visits']
        if n+r > 65536 or (obj['injected'] and (n != occurrence or obj['command_succeeded'])) or \
           (not obj['injected'] and n >= occurrence):
            raise ValueError()
        return obj
    except (ValueError, TypeError, KeyError):
        raise ValueError('F14_REPORT_OUTCOME_UNKNOWN') from None


def run(staged, arguments, audit, *, occurrence, evidence_root,
        enable=False, stop=lambda: False, timeout=5, lifetime=60):
    """Authenticated correction_stage scope only. No retry or evidence removal.

    START may persist an observation and execute Apply. A valid child report
    describes its result; it does not certify F14, replay or runtime approval.
    """
    options = correction_arguments(occurrence, evidence_root, enable)
    arguments = list(arguments)
    if any(not isinstance(arg, str) or arg.startswith(('--fault-', '--enable-f14',
           '--enable-storage')) or arg == '--worker-inputs' for arg in arguments):
        raise ValueError('DUPLICATE_F14_OPTIONS')
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 60:
        raise ValueError('F14_LIFETIME_LIMIT')
    with _ready_scope(staged, options+arguments, audit, timeout, stop,
                      command='f14-apply-captured') as (child, gate, baseline, deadline):
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
                        if len(output) > 1024:
                            raise ValueError('FAULT_REPORT_OUTCOME_UNKNOWN')
                code = child.poll()
                if code is not None and not selector.get_map():
                    result = decode_report(bytes(output), occurrence)
                    if code != 0 or result is None:
                        raise ValueError('FAULT_FAILED_OUTCOME_UNKNOWN')
                    return dict(child_result=result, child_exit=0, fault_started=True,
                                approval_verified=False, reusable_permit=False,
                                replay_verified=False, F14_verified=False)
