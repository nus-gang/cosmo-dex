"""Bounded bootstrap fetch child IO. Caller owns approval and byte pinning."""
import hashlib
import ipaddress
import math
import os
import selectors
import signal
import subprocess
import time
from pathlib import Path
from process_check import MAX_INPUT

MAX_RPC = 16 * 1024 * 1024
MAX_ERROR = 4096


class FetchFailure(ValueError):
    """Untrusted partial evidence; never a successful Snapshot or create permit."""
    def __init__(self, code, raw=b''):
        super().__init__(code)
        self.partial_raw = bytes(raw)


def fetch_captured(executable, address, pin, profile, acknowledge, raw,
                   timeout=10, stopped=lambda: False):
    """No shell, ambient credentials, retries, parsing, or home creation.

    The caller must stage exact reviewed executable bytes and freshly audit
    immediately before calling. Returned bytes still require durable evidence
    publication, C semantic validation and a separate create approval check.
    """
    if not isinstance(raw, bytes) or not 0 < len(raw) <= MAX_INPUT:
        raise FetchFailure('FETCH_CAPTURE_SIZE')
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 60:
        raise FetchFailure('FETCH_TIME_LIMIT')
    if acknowledge is not True or not isinstance(pin, str) or len(pin) != 64 or any(c not in '0123456789abcdef' for c in pin):
        raise FetchFailure('FETCH_OPT_IN')
    for path in (executable, profile):
        if not isinstance(path, (str, Path)) or not Path(path).is_absolute() or '..' in Path(path).parts:
            raise FetchFailure('FETCH_PATH')
    try:
        host, port = address.rsplit(':', 1)
        ip = ipaddress.ip_address(host[1:-1] if host.startswith('[') and host.endswith(']') else host)
        number = int(port)
        canonical = f'[{ip}]:{number}' if ip.version == 6 else f'{ip}:{number}'
        if not ip.is_loopback or not 1024 <= number <= 65535 or address != canonical:
            raise ValueError()
    except (ValueError, AttributeError, TypeError):
        raise FetchFailure('FETCH_ADDRESS') from None
    if stopped():
        raise FetchFailure('FETCH_STOPPED')
    deadline = time.monotonic() + timeout
    argv = [str(executable), 'fetch-captured', '--chain-rpc', address,
            '--capture-sha256', hashlib.sha256(raw).hexdigest(), '--runtime-pin', pin,
            '--local-demo-profile', str(profile), '--acknowledge-unproven-space']
    output = bytearray()
    errors = bytearray()
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, start_new_session=True, close_fds=True,
        env={'LANG': 'C', 'LC_ALL': 'C'})
    offset = 0
    try:
        with selectors.DefaultSelector() as selector:
            for stream, name, event in ((child.stdin, 'in', selectors.EVENT_WRITE),
                    (child.stdout, 'out', selectors.EVENT_READ), (child.stderr, 'err', selectors.EVENT_READ)):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, event, name)
            while selector.get_map():
                if stopped():
                    raise FetchFailure('FETCH_STOPPED', output)
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise FetchFailure('FETCH_TIMEOUT', output)
                for key, _ in selector.select(min(remaining, .05)):
                    stream, name = key.fileobj, key.data
                    if name == 'in':
                        try:
                            offset += os.write(stream.fileno(), raw[offset:offset + 65536])
                        except BrokenPipeError:
                            raise FetchFailure('FETCH_INPUT_CLOSED', output) from None
                        if offset == len(raw):
                            selector.unregister(stream)
                            stream.close()
                    else:
                        target, cap = (output, MAX_RPC) if name == 'out' else (errors, MAX_ERROR)
                        data = os.read(stream.fileno(), min(65536, cap - len(target) + 1))
                        if not data:
                            selector.unregister(stream)
                            stream.close()
                        elif len(target) + len(data) > cap:
                            target.extend(data[:cap-len(target)])
                            raise FetchFailure('FETCH_OUTPUT_LIMIT', output)
                        else:
                            target.extend(data)
            while child.poll() is None:
                if stopped():
                    raise FetchFailure('FETCH_STOPPED', output)
                if time.monotonic() >= deadline:
                    raise FetchFailure('FETCH_TIMEOUT', output)
                time.sleep(.01)
        if stopped():
            raise FetchFailure('FETCH_STOPPED', output)
        if child.returncode != 0 or offset != len(raw) or errors or not output:
            raise FetchFailure('FETCH_REJECTED', output)
        return bytes(output)
    finally:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait()
        for stream in (child.stdin, child.stdout, child.stderr):
            stream.close()
