"""Bounded bootstrap fetch child IO. Caller owns approval and byte pinning."""
import hashlib
import ipaddress
import math
import json
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
                   timeout=10, stopped=lambda: False, trace=None):
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
    pipes = []
    diagnostics = bytearray(); diagnostic_total = 0; wire_total = 0
    transport_complete = False; last_elapsed = 0; received_bytes=0
    def evidence(name, data):
        nonlocal diagnostic_total, wire_total, transport_complete, last_elapsed, received_bytes
        if name == 'wire':
            wire_total += len(data)
            trace.wire(data)
            return
        diagnostic_total += len(data)
        if diagnostic_total > 1048576: raise FetchFailure('FETCH_OUTPUT_LIMIT', output)
        diagnostics.extend(data)
        while b'\n' in diagnostics:
            line, _, rest = diagnostics.partition(b'\n'); diagnostics[:] = rest
            try:
                value = json.loads(line)
                if set(value) != {'stage','code','sent_bytes','received_bytes','child_elapsed_ns'}:
                    raise ValueError()
                if value['stage'] not in ('child_capture','request_start','connected','sent','received','entity_complete','failed'):
                    raise ValueError()
                if value['child_elapsed_ns'] < last_elapsed: raise ValueError()
                trace.emit(**value)
                last_elapsed = value['child_elapsed_ns']
                received_bytes = value['received_bytes']
                transport_complete = value['stage'] == 'entity_complete'
            except (ValueError, TypeError, KeyError):
                raise FetchFailure('FETCH_REJECTED', output) from None
    child = None
    try:
        if trace is not None:
            pipes.append(os.pipe());pipes.append(os.pipe())
            argv[4:4] = ['--diagnostic-fd', str(pipes[0][1]), '--wire-fd', str(pipes[1][1])]
        child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, start_new_session=True, close_fds=True,
            pass_fds=tuple(p[1] for p in pipes), env={'LANG': 'C', 'LC_ALL': 'C'})
    finally:
        for _, fd in pipes: os.close(fd)
        if child is None:
            for fd, _ in pipes: os.close(fd)
    offset = 0
    extra_streams = [os.fdopen(fd, 'rb', buffering=0) for fd, _ in pipes]
    primary=None
    try:
        if trace is not None: trace.emit('child_spawn', pid=child.pid)
        with selectors.DefaultSelector() as selector:
            for stream, name, event in ((child.stdin, 'in', selectors.EVENT_WRITE),
                    (child.stdout, 'out', selectors.EVENT_READ), (child.stderr, 'err', selectors.EVENT_READ)):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, event, name)
            for stream, name in zip(extra_streams, ('diagnostic','wire')):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, name)
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
                    elif name in ('diagnostic','wire'):
                        data = os.read(stream.fileno(), 65536)
                        if not data:
                            selector.unregister(stream); stream.close()
                        else: evidence(name, data)
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
        if child.returncode != 0 or offset != len(raw) or errors or not output or (trace is not None and (diagnostics or not transport_complete or received_bytes!=wire_total)):
            raise FetchFailure('FETCH_REJECTED', output)
        return bytes(output)
    except BaseException as error:
        primary=error
        error.partial_raw = bytes(output)
        raise
    finally:
        if child.poll() is None:
            try:os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:pass
        child.wait(timeout=3)
        # Drain already received pipe bytes after kill/reap; do not wait/retry
        # the RPC, interpret stderr, or promote incomplete wire to an entity.
        for stream, name in zip(extra_streams, ('diagnostic','wire')):
            if not stream.closed:
                try:
                    while True:
                        data = os.read(stream.fileno(), 65536)
                        if not data: break
                        evidence(name, data)
                except (OSError, ValueError): pass
                finally: stream.close()
        for stream in (child.stdin, child.stdout, child.stderr): stream.close()
        if trace is not None:
            try:trace.emit('child_exit', pid=child.pid, exit_code=child.returncode)
            except BaseException as error:
                if primary is None:
                    error.partial_raw=bytes(output)
                    raise
