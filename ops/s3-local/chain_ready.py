"""B preflight and offline Chain READY probe. Never writes START."""
from contextlib import contextmanager
import math
import os
import selectors
import signal
import subprocess
import time
import chain_preflight


@contextmanager
def _scope(staged, pin, home, rpc, p2p, peers, audit, *, stopped=lambda: False,
          timeout=30, on_spawn=None):
    """Keep chain_stage alive; audit closes over the exact candidate/revisions.

    READY owns a writer lock, but creates no DB/node. The lock inode can remain
    after rejection. Caller owns home/evidence retention. This is no permit.
    """
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 30:
        raise ValueError('CHAIN_READY_TIME_LIMIT')
    if stopped():
        raise ValueError('CHAIN_READY_STOPPED')
    baseline = audit()
    chain_preflight.check(staged, pin, home, rpc, p2p, peers, stopped=stopped)
    if audit() != baseline:
        raise ValueError('APPROVAL_CHANGED_BEFORE_CHAIN_READY')
    staged.verify()
    if stopped():
        raise ValueError('CHAIN_READY_STOPPED')
    argv = chain_preflight.arguments(staged, pin, home, rpc, p2p, peers, mode='start')
    deadline = time.monotonic() + timeout
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, start_new_session=True,
                             env={'LANG': 'C', 'LC_ALL': 'C'}, close_fds=True)
    try:
        if on_spawn is not None:
            on_spawn(child.pid)
        received = bytearray()
        with selectors.DefaultSelector() as selector:
            for stream in (child.stdout, child.stderr):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ)
            while received != b'CHAIN_READY\n':
                if stopped():
                    raise ValueError('CHAIN_READY_STOPPED')
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ValueError('CHAIN_READY_TIMEOUT')
                for key, _ in selector.select(min(remaining, 0.05)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data or key.fileobj is child.stderr:
                        raise ValueError('CHAIN_READY_REJECTED')
                    received.extend(data)
                    if not b'CHAIN_READY\n'.startswith(received):
                        raise ValueError('CHAIN_READY_PROTOCOL')
        if audit() != baseline:
            raise ValueError('APPROVAL_CHANGED_AFTER_CHAIN_READY')
        staged.verify()
        if stopped() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('CHAIN_READY_EXPIRED')
        yield child, baseline, deadline
    finally:
        # Close the handshake and unconditionally reap our process group.
        child.stdin.close()
        try:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        finally:
            # Even a host signal-denial must not skip wait/pipe cleanup.
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=5)
            finally:
                child.stdout.close()
                child.stderr.close()


@contextmanager
def ready(staged, pin, home, rpc, p2p, peers, audit, **kwargs):
    """Offline probe only: never expose the internal child or transmit START."""
    with _scope(staged, pin, home, rpc, p2p, peers, audit, **kwargs):
        yield {'ready': True, 'service_started': False, 'start_sent': False,
               'approval_verified': False, 'reusable_permit': False}
