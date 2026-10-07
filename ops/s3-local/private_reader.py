"""Bounded Unix IPC for a current-run read-only Paperclip reader.

The bearer stays in the broker process. Same-uid processes are trusted, as with
other local runtime inputs; directory modes are not protection from that uid.
No cached approvals, token transfer, API writes, or service launch.
"""
from contextlib import contextmanager
import os
from pathlib import Path
import socket
import stat
import struct
import tempfile
import threading
import time

from manifest import decode, encode
from paperclip_reader import Reader, PATHS, MAX_RESPONSE

ERROR = 'PRIVATE_APPROVAL_READ_FAILED'


def _receive(conn, limit, deadline):
    def exact(size):
        result = bytearray()
        while len(result) < size:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError(ERROR)
            conn.settimeout(remaining)
            data = conn.recv(min(size - len(result), 65536))
            if not data:
                raise ValueError(ERROR)
            result.extend(data)
        return bytes(result)
    size = struct.unpack('!I', exact(4))[0]
    if not 0 < size <= limit:
        raise ValueError(ERROR)
    raw = exact(size)
    conn.settimeout(max(0.001, deadline - time.monotonic()))
    if conn.recv(1):
        raise ValueError(ERROR)
    return raw


def _send(conn, raw, limit, deadline):
    if not isinstance(raw, bytes) or not 0 < len(raw) <= limit:
        raise ValueError(ERROR)
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise ValueError(ERROR)
    conn.settimeout(remaining)
    conn.sendall(struct.pack('!I', len(raw)) + raw)
    conn.shutdown(socket.SHUT_WR)


def _serve(conn, reader, deadline):
    # One request and one fresh upstream GET per connection, never batching.
    path = _receive(conn, 512, min(deadline, time.monotonic() + 2)).decode('ascii')
    if path not in PATHS:
        raise ValueError(ERROR)
    result = reader(path)
    if not isinstance(result, dict):
        raise ValueError(ERROR)
    _send(conn, encode(result), MAX_RESPONSE, min(deadline, time.monotonic() + 2))


class PrivateReader:
    def __init__(self, endpoint):
        self.endpoint = Path(endpoint)

    def __call__(self, path):
        try:
            p = self.endpoint
            if path not in PATHS or not p.is_absolute() or '..' in p.parts:
                raise ValueError()
            parent, node = p.parent.lstat(), p.lstat()
            if (not stat.S_ISDIR(parent.st_mode) or stat.S_IMODE(parent.st_mode) != 0o700 or
                    parent.st_uid != os.getuid() or not stat.S_ISSOCK(node.st_mode) or
                    stat.S_IMODE(node.st_mode) != 0o600 or node.st_uid != os.getuid()):
                raise ValueError()
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
                conn.settimeout(2)
                conn.connect(str(p))
                # Refuse path replacement across connect. Same-uid malicious
                # replacement is outside this local runtime threat boundary.
                for path_, before in ((p.parent, parent), (p, node)):
                    after = path_.lstat()
                    if (after.st_dev, after.st_ino, after.st_mode, after.st_uid) != \
                       (before.st_dev, before.st_ino, before.st_mode, before.st_uid):
                        raise ValueError()
                deadline = time.monotonic() + 12
                _send(conn, path.encode('ascii'), 512, deadline)
                result = decode(_receive(conn, MAX_RESPONSE, deadline))
                if not isinstance(result, dict):
                    raise ValueError()
                return result
        except Exception:
            raise ValueError(ERROR) from None


@contextmanager
def _broker(scratch, reader, *, lifetime=300, max_requests=128, fixed_root=None):
    """Internal test seam. Public broker always uses current-run authentication."""
    if type(lifetime) is not int or not 1 <= lifetime <= 300 or \
       type(max_requests) is not int or not 1 <= max_requests <= 128:
        raise ValueError(ERROR)
    if fixed_root is None:
        root = Path(tempfile.mkdtemp(prefix='a', dir=scratch))
    else:
        root = Path(fixed_root)
        parent = root.parent
        meta = parent.lstat()
        if (not root.is_absolute() or '..' in root.parts or
                parent.resolve(strict=True) != parent or
                not stat.S_ISDIR(meta.st_mode) or
                stat.S_IMODE(meta.st_mode) != 0o700 or meta.st_uid != os.getuid() or
                len(os.fsencode(root / 's')) > 103):
            raise ValueError(ERROR)
        # Atomic lease: no takeover, stale cleanup, or replacement of any node.
        root.mkdir(mode=0o700)
    identity = root.lstat()
    endpoint = root / 's'
    listener = None
    stopped = threading.Event()
    thread = None
    try:
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(endpoint))
        os.chmod(endpoint, 0o600)
        listener.listen(1)
        listener.settimeout(0.1)
        deadline = time.monotonic() + lifetime
        def serve():
            count = 0
            while not stopped.is_set() and time.monotonic() < deadline and count < max_requests:
                try:
                    conn, _ = listener.accept()
                except socket.timeout:
                    continue
                except OSError:
                    return
                count += 1  # Invalid requests consume the same finite budget.
                with conn:
                    try:
                        _serve(conn, reader, deadline)
                    except Exception:
                        pass  # No response body, credential or exception in logs.
        thread = threading.Thread(target=serve, daemon=False)
        thread.start()
        yield endpoint
    finally:
        stopped.set()
        if listener is not None:
            listener.close()
        if thread is not None:
            thread.join()  # Production Reader and every IPC operation are bounded.
        current = root.lstat()
        if (current.st_dev, current.st_ino) != (identity.st_dev, identity.st_ino):
            raise ValueError(ERROR)
        # Never recursively remove unexpected files, including diagnostic data.
        if endpoint.exists():
            endpoint.unlink()
        root.rmdir()


@contextmanager
def broker(scratch, *, lifetime=300, max_requests=128):
    reader = Reader.from_environment()
    with _broker(scratch, reader, lifetime=lifetime, max_requests=max_requests) as endpoint:
        yield endpoint


@contextmanager
def broker_at(root, *, lifetime=300, max_requests=128):
    """Current-run authenticated broker at a pre-registered fixed root/s path.

    Parent must be a dedicated canonical private directory. Existing roots
    (including stale crash remnants) fail closed; cleanup is operator-reviewed.
    This grants neither command registration permission nor runtime approval.
    """
    reader = Reader.from_environment()
    with _broker(None, reader, lifetime=lifetime, max_requests=max_requests,
                 fixed_root=root) as endpoint:
        yield endpoint
