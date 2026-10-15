"""Bounded B preflight for an in-scope StagedChain; never starts a node."""
import math
import os
import re
import selectors
import signal
import subprocess
import time

EXPECTED = b'VALIDATED_INPUT_BYTES_AND_HOME_ONLY; RUNTIME_APPROVAL_NOT_ESTABLISHED; durable_ack=false\n'


def check(staged, pin, home, rpc, p2p, peers='', *, timeout=60, stopped=lambda: False):
    if (type(timeout) not in (int, float) or not math.isfinite(timeout)
            or not 0 < timeout <= 60 or not isinstance(pin, str)
            or re.fullmatch('[0-9a-f]{64}', pin) is None):
        raise ValueError('CHAIN_PREFLIGHT_ARGUMENTS')
    home = os.fspath(home)
    if not os.path.isabs(home) or os.path.normpath(home) != home:
        raise ValueError('CHAIN_HOME_PATH')
    if any(not isinstance(v, str) or '\x00' in v for v in (rpc, p2p, peers)):
        raise ValueError('CHAIN_PREFLIGHT_ARGUMENTS')
    argv = arguments(staged, pin, home, rpc, p2p, peers)
    _supervise(argv, staged.verify, EXPECTED, timeout=timeout, stopped=stopped)
    return {'b_preflight': True, 'approval_verified': False,
            'service_started': False, 'durable_ack': False}


def _supervise(argv, verify, expected, *, timeout, stopped):
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 60:
        raise ValueError('CHAIN_PREFLIGHT_ARGUMENTS')
    verify()
    if stopped():
        raise ValueError('CHAIN_PREFLIGHT_STOPPED')
    deadline = time.monotonic() + timeout
    child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, start_new_session=True,
                             env={'LANG': 'C', 'LC_ALL': 'C'}, close_fds=True)
    output = {'stdout': bytearray(), 'stderr': bytearray()}
    try:
        with selectors.DefaultSelector() as selector:
            for stream, name in ((child.stdout, 'stdout'), (child.stderr, 'stderr')):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, name)
            while selector.get_map() or child.poll() is None:
                if stopped():
                    raise ValueError('CHAIN_PREFLIGHT_STOPPED')
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ValueError('CHAIN_PREFLIGHT_TIMEOUT')
                for key, _ in selector.select(min(remaining, 0.05)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    else:
                        output[key.data].extend(data)
                        if len(output[key.data]) > 4096:
                            raise ValueError('CHAIN_PREFLIGHT_OUTPUT_LIMIT')
        if child.wait() != 0 or output['stderr'] or bytes(output['stdout']) != expected:
            raise ValueError('CHAIN_PREFLIGHT_REJECTED')
        verify()
        if stopped():
            raise ValueError('CHAIN_PREFLIGHT_STOPPED')
    finally:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait()
        child.stdout.close()
        child.stderr.close()


def arguments(staged, pin, home, rpc, p2p, peers="", *, mode="preflight"):
    if mode not in ("preflight", "start"):
        raise ValueError("CHAIN_MODE")
    argv = [str(staged.executable), mode, '--local-demo-profile',
            str(staged.effective_profile), '--acknowledge-unproven-space',
            '--input-set', str(staged.input_set), '--runtime-pin', pin,
            '--home', home, '--rpc', rpc, '--p2p', p2p]
    if peers:
        argv += ['--peers', peers]
    return argv
