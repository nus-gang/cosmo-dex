"""READY probe and foreground managed-worker supervision; no reusable permit."""
from contextlib import contextmanager
import hashlib
import math
import os
import selectors
import signal
import socket
import subprocess
import time
from offline_check import MAX_BINARY
from preflight import bounded, checked_root
from process_check import MAX_INPUT, MAX_OUTPUT


def check_bytes(staged):
    raw = bounded(checked_root(staged.executable.parent), staged.executable.name, MAX_BINARY)
    if hashlib.sha256(raw).hexdigest() != staged.executable_sha256:
        raise ValueError('STAGED_WORKER_BYTES_CHANGED')
    if not isinstance(staged.capture, bytes) or not 0 < len(staged.capture) <= MAX_INPUT or \
       hashlib.sha256(staged.capture).hexdigest() != staged.capture_sha256:
        raise ValueError('CAPTURE_CHANGED')


@contextmanager
def _ready_scope(staged, arguments, audit, timeout=60, stop=lambda: False, on_spawn=None, *,
                 command="serve-captured", prefix=()):
    """Own child until scope exit; caller must keep validated_stage alive.

    audit is a trusted closure over exact candidate, decision and revisions.
    A later managed launch operation must recheck immediately before START.
    Private scope shares the channel only with the two wrappers below.
    """
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 60:
        raise ValueError('READY_TIME_LIMIT')
    if stop():
        raise ValueError("WORKER_STOPPED_BEFORE_START")
    before = audit()
    check_bytes(staged)
    deadline = time.monotonic() + timeout
    parent, gate = socket.socketpair()
    child = None
    try:
        child = subprocess.Popen([str(staged.executable), command, '--start-gate-fd',
            str(gate.fileno()), *prefix, '--capture-sha256', staged.capture_sha256, *list(arguments)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            pass_fds=(gate.fileno(),), close_fds=True, start_new_session=True,
            env={'LANG': 'C', 'LC_ALL': 'C'})
        if on_spawn is not None:
            on_spawn(child.pid)
        gate.close()
        offset = 0
        received = bytearray()
        output = {'stdout': 0, 'stderr': 0}
        with selectors.DefaultSelector() as selector:
            for stream, name, events in [(child.stdin, 'stdin', selectors.EVENT_WRITE),
                    (child.stdout, 'stdout', selectors.EVENT_READ),
                    (child.stderr, 'stderr', selectors.EVENT_READ),
                    (parent, 'gate', selectors.EVENT_READ)]:
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, events, name)
            while received != b'READY\n':
                if stop():
                    raise ValueError('WORKER_STOPPED_BEFORE_START')
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ValueError('WORKER_READY_TIMEOUT')
                for key, _ in selector.select(min(0.05, remaining)):
                    stream, name = key.fileobj, key.data
                    if name == 'stdin':
                        try:
                            offset += os.write(stream.fileno(), staged.capture[offset:offset+65536])
                        except BrokenPipeError:
                            raise ValueError('WORKER_INPUT_CLOSED') from None
                        if offset == len(staged.capture):
                            selector.unregister(stream)
                            stream.close()
                    else:
                        data = os.read(stream.fileno(), 4096)
                        if not data:
                            raise ValueError('WORKER_EARLY_EXIT')
                        if name == 'gate':
                            received.extend(data)
                            if not b'READY\n'.startswith(received):
                                raise ValueError('WORKER_READY_PROTOCOL')
                        else:
                            output[name] += len(data)
                            if output[name] > MAX_OUTPUT:
                                raise ValueError('WORKER_OUTPUT_LIMIT')
                            # Real worker is silent until rejection; never relay diagnostics.
                            raise ValueError('WORKER_UNEXPECTED_OUTPUT')
        if offset != len(staged.capture) or child.poll() is not None:
            raise ValueError('WORKER_NOT_READY')
        after = audit()
        check_bytes(staged)
        if before != after:
            raise ValueError('APPROVAL_CHANGED_AFTER_READY')
        if time.monotonic() >= deadline or child.poll() is not None:
            raise ValueError('WORKER_READY_EXPIRED')
        yield child, parent, before, deadline
    finally:
        parent.close()
        gate.close()
        if child is not None:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            for stream in (child.stdin, child.stdout, child.stderr):
                stream.close()


@contextmanager
def ready(staged, arguments, audit, timeout=60, *, on_spawn=None):
    """Offline READY probe: never transmits START."""
    with _ready_scope(staged, arguments, audit, timeout, on_spawn=on_spawn):
        yield {'ready': True, 'service_started': False, 'approval_verified': False,
               'reusable_permit': False}


def run_managed(staged, arguments, audit, stop, timeout=60, lifetime=300, *, on_spawn=None):
    """Foreground child supervision for a Paperclip-managed launcher.

    Only call inside validated_stage, with an authenticated exact-candidate audit.
    This internal function does not establish Paperclip runtime ownership. Its
    caller must be the configured managed service. No restart or retry occurs.
    stop is a signal latch callback; errors/interrupts always reap the child.
    """
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 300:
        raise ValueError('WORKER_LIFETIME_LIMIT')
    if stop():
        raise ValueError('WORKER_STOPPED_BEFORE_START')
    with _ready_scope(staged, arguments, audit, timeout, stop, on_spawn) as (child, gate, baseline, deadline):
        # Do not reuse the READY report as a permit. Read live approval again,
        # then check this private executable and the exact captured input bytes.
        if audit() != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_START')
        check_bytes(staged)
        if stop() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('WORKER_STOPPED_BEFORE_START')
        gate.settimeout(min(1, max(0.001, deadline-time.monotonic())))
        gate.sendall(b'START\n')
        gate.shutdown(socket.SHUT_WR)
        end = time.monotonic() + lifetime
        with selectors.DefaultSelector() as selector:
            for stream in (child.stdout, child.stderr):
                selector.register(stream, selectors.EVENT_READ)
            while True:
                if stop():
                    raise ValueError('WORKER_STOPPED')
                remaining = end - time.monotonic()
                if remaining <= 0:
                    raise ValueError('WORKER_LIFETIME_EXCEEDED')
                for key, _ in selector.select(min(0.05, remaining)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if data:
                        # Never forward secrets or unbounded child diagnostics.
                        raise ValueError('WORKER_UNEXPECTED_OUTPUT')
                    selector.unregister(key.fileobj)
                code = child.poll()
                if code is not None and not selector.get_map():
                    if code != 0:
                        raise ValueError('WORKER_FAILED')
                    return {'worker_exit': 0, 'reusable_permit': False}
