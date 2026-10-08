"""One-shot local PID handoff. Same uid is trusted; never a launch permit.

Use a fresh private directory owned by one managed session. A random challenge
binds the report to that session. Missing/partial/replaced reports fail closed.
Caller preserves ambiguous directories for inspection, never repairs them.
"""
import json
import os
from pathlib import Path
import secrets
import stat

ERROR = 'PID_HANDOFF_REJECTED'
LIMIT = 512


def _root(path):
    path = Path(path)
    m = path.lstat()
    if (not path.is_absolute() or '..' in path.parts or path.resolve(strict=True) != path or
            not stat.S_ISDIR(m.st_mode) or stat.S_IMODE(m.st_mode) != 0o700 or
            m.st_uid != os.getuid()):
        raise ValueError(ERROR)
    return path


def _read(root, name):
    fd = os.open(root / name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        m = os.fstat(fd)
        if (not stat.S_ISREG(m.st_mode) or stat.S_IMODE(m.st_mode) != 0o600 or
                m.st_uid != os.getuid() or m.st_nlink != 1 or not 0 < m.st_size <= LIMIT):
            raise ValueError(ERROR)
        raw = os.read(fd, LIMIT + 1)
        if len(raw) != m.st_size or os.read(fd, 1):
            raise ValueError(ERROR)
        after = (root / name).lstat()
        if (after.st_dev, after.st_ino) != (m.st_dev, m.st_ino):
            raise ValueError(ERROR)
        return raw
    finally:
        os.close(fd)


def _write(root, name, raw):
    # Exclusive creation: partial writes remain evidence and are never replaced.
    fd = os.open(root / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        os.fchmod(fd, 0o600)
        if os.write(fd, raw) != len(raw):
            raise ValueError(ERROR)
        os.fsync(fd)
    finally:
        os.close(fd)
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)


def _nonce(raw):
    value = raw.decode('ascii')
    if len(value) != 64 or any(c not in '0123456789abcdef' for c in value):
        raise ValueError(ERROR)
    return value


def reporter(root):
    """Create on_spawn callback before launch; it writes before READY/START.

    The orchestrator must keep this directory until after confirmed stop. A
    duplicate callback or write error poisons this reporter permanently.
    """
    try:
        root = _root(root)
        nonce = _nonce(_read(root, 'challenge'))
        identity = root.stat()
    except Exception:
        raise ValueError(ERROR) from None
    used = False
    def record(pid):
        nonlocal used
        if used:
            raise ValueError(ERROR)
        used = True
        try:
            current = _root(root).stat()
            if (current.st_dev, current.st_ino) != (identity.st_dev, identity.st_ino):
                raise ValueError()
            if (_nonce(_read(root, 'challenge')) != nonce or type(pid) is not int or
                    not 2 <= pid <= 2147483647 or pid == os.getpid()):
                raise ValueError()
            raw = json.dumps({'schema': 's3-local-pids/1', 'nonce': nonce,
                              'launcher_pid': os.getpid(), 'worker_pid': pid},
                             sort_keys=True, separators=(',', ':')).encode()
            _write(root, 'pids.json', raw)
        except Exception:
            raise ValueError(ERROR) from None
    return record


class Mailbox:
    def __init__(self, path):
        try:
            path = Path(path)
            _root(path.parent)
            path.mkdir(mode=0o700)
            self.root = _root(path)
            self.identity = self.root.stat()
            self.nonce = secrets.token_hex(32)
            self.used = False
            _write(self.root, 'challenge', self.nonce.encode('ascii'))
        except Exception:
            raise ValueError(ERROR) from None

    def collect(self):
        if self.used:
            raise ValueError(ERROR)
        self.used = True
        try:
            m = _root(self.root).stat()
            if (m.st_dev, m.st_ino) != (self.identity.st_dev, self.identity.st_ino):
                raise ValueError()
            if _nonce(_read(self.root, 'challenge')) != self.nonce:
                raise ValueError()
            raw = _read(self.root, 'pids.json')
            report = json.loads(raw)
            if (set(report) != {'schema','nonce','launcher_pid','worker_pid'} or
                    report['schema'] != 's3-local-pids/1' or report['nonce'] != self.nonce or
                    json.dumps(report, sort_keys=True, separators=(',', ':')).encode() != raw):
                raise ValueError()
            pids = (report['launcher_pid'], report['worker_pid'])
            if (any(type(p) is not int or not 2 <= p <= 2147483647 for p in pids) or
                    pids[0] == pids[1]):
                raise ValueError()
            return pids
        except Exception:
            raise ValueError(ERROR) from None
