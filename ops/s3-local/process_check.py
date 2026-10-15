#!/usr/bin/env python3
"""Bounded offline validator supervision; not a service/approval launcher."""
import hashlib
import os
import selectors
import signal
import subprocess
import time
from manifest import decode

MAX_INPUT = 48 * 1024 * 1024
MAX_OUTPUT = 4096
EXPECTED = {'semantic_validation': True, 'approval_verified': False,
            'service_started': False, 'durable_ack': False}


def validate_captured(executable, arguments, raw, timeout=60):
    """Caller supplies an independently byte-checked offline validator.

    No shell/env forwarding, bounded stdin/stdout/stderr, and no live producer.
    This API does not approve or resolve executable paths. The final launcher
    must bind the executable to its reviewed descriptor before calling it.
    """
    if not isinstance(raw, bytes) or not 0 < len(raw) <= MAX_INPUT:
        raise ValueError('CAPTURE_SIZE')
    if not isinstance(timeout, (int, float)) or not 0 < timeout <= 60:
        raise ValueError('TIME_LIMIT')
    if not os.path.isabs(executable):
        raise ValueError('EXECUTABLE_PATH')
    deadline = time.monotonic() + timeout
    argv = [executable, 'validate-captured', '--capture-sha256',
            hashlib.sha256(raw).hexdigest(), *arguments]
    # A fresh process group allows deterministic cleanup even after EOF or
    # early child exit. No Paperclip credentials or loader variables inherited.
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, start_new_session=True,
                             env={'LANG': 'C', 'LC_ALL': 'C'}, close_fds=True)
    output = {'stdout': bytearray(), 'stderr': bytearray()}
    offset = 0
    try:
        with selectors.DefaultSelector() as selector:
            for stream, name, event in ((child.stdin, 'stdin', selectors.EVENT_WRITE),
                                       (child.stdout, 'stdout', selectors.EVENT_READ),
                                       (child.stderr, 'stderr', selectors.EVENT_READ)):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, event, name)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ValueError('VALIDATOR_TIMEOUT')
                for key, _ in selector.select(remaining):
                    stream, name = key.fileobj, key.data
                    if name == 'stdin':
                        try:
                            offset += os.write(stream.fileno(), raw[offset:offset + 65536])
                        except BrokenPipeError:
                            raise ValueError('VALIDATOR_INPUT_CLOSED') from None
                        if offset == len(raw):
                            selector.unregister(stream)
                            stream.close()
                    else:
                        data = os.read(stream.fileno(), 4096)
                        if not data:
                            selector.unregister(stream)
                            stream.close()
                        else:
                            output[name].extend(data)
                            if len(output[name]) > MAX_OUTPUT:
                                raise ValueError('VALIDATOR_OUTPUT_LIMIT')
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError('VALIDATOR_TIMEOUT')
            try:
                code = child.wait(timeout=remaining)
            except subprocess.TimeoutExpired:
                raise ValueError('VALIDATOR_TIMEOUT') from None
        if code != 0 or offset != len(raw) or output['stderr']:
            raise ValueError('VALIDATOR_REJECTED')
        # Type-exact wire comparison: Python True == 1 must not allow numbers.
        value = decode(bytes(output['stdout']))
        if not isinstance(value, dict) or set(value) != set(EXPECTED) or any(
                value[k] is not v for k, v in EXPECTED.items()):
            raise ValueError('VALIDATOR_REPORT')
        return dict(EXPECTED)
    finally:
        # Kill and reap on success, failure, KeyboardInterrupt and parser errors.
        # No home, key, guard, WAL, writer-lock inode or evidence is removed.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait()
        for stream in (child.stdin, child.stdout, child.stderr):
            stream.close()
