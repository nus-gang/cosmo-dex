"""Scoped foreground launcher signals; runtime ownership is supplied by Paperclip."""
from contextlib import contextmanager
import signal
import threading


@contextmanager
def stop_latch():
    if threading.current_thread() is not threading.main_thread():
        raise ValueError('LAUNCHER_MAIN_THREAD_REQUIRED')
    signals = (signal.SIGINT, signal.SIGTERM)
    previous = {sig: signal.getsignal(sig) for sig in signals}
    if previous[signal.SIGINT] not in (signal.SIG_DFL, signal.default_int_handler) or \
       previous[signal.SIGTERM] != signal.SIG_DFL:
        raise ValueError('LAUNCHER_SIGNAL_CONFLICT')
    stopped = False
    installed = []
    def latch(signum, frame):
        nonlocal stopped
        stopped = True
    try:
        for sig in signals:
            signal.signal(sig, latch)
            installed.append(sig)
        yield lambda: stopped
    finally:
        for sig in reversed(installed):
            signal.signal(sig, previous[sig])


def run(*args, **kwargs):
    """L-T only: scoped signals cover validation, READY and foreground execution.

    This does not register a managed service, authenticate runtime ownership or
    authorize a start. checked_run retains exact-candidate approval enforcement.
    """
    from staged_worker import checked_run
    if 'stop' in kwargs:
        raise ValueError('LAUNCHER_STOP_OVERRIDE')
    with stop_latch() as stop:
        return checked_run(*args, stop=stop, **kwargs)
