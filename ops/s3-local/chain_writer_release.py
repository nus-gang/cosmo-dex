"""Observe an existing Chain flock once; never create, truncate or unlink it.

The lock is released before return. This is not a continuing exclusion guarantee,
proof of a complete process inventory, or proof of filesystem durability.
"""
import fcntl
import os
from pathlib import Path
import stat
import time

ERROR = 'CHAIN_WRITER_RELEASE_UNCONFIRMED'
NAME = 'writer.dev.lock'


def check(home):
    return _check(home, fcntl.flock, time.monotonic_ns)


def _check(home, flock, clock):
    root_fd = lock_fd = None
    try:
        home = Path(home)
        if not home.is_absolute() or str(home.resolve(strict=True)) != str(home):
            raise ValueError()
        root_fd = os.open(home, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        root = os.fstat(root_fd)
        if stat.S_IMODE(root.st_mode) != 0o700 or root.st_uid != os.getuid():
            raise ValueError()
        lock_fd = os.open(NAME, os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=root_fd)
        before = os.fstat(lock_fd)
        if (not stat.S_ISREG(before.st_mode) or stat.S_IMODE(before.st_mode) != 0o600 or
                before.st_uid != os.getuid() or before.st_nlink != 1):
            raise ValueError()
        started = clock()
        if type(started) is not int or started < 0:
            raise ValueError()
        flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        current = os.stat(NAME, dir_fd=root_fd, follow_symlinks=False)
        after = os.fstat(lock_fd)
        path_root = os.stat(home, follow_symlinks=False)
        identity = lambda s: (s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_nlink,
                              s.st_size, s.st_mtime_ns, s.st_ctime_ns)
        if (identity(before) != identity(after) or identity(after) != identity(current) or
                identity(root) != identity(path_root) or home.resolve(strict=True) != home):
            raise ValueError()
        finished = clock()
        if type(finished) is not int or finished < started:
            raise ValueError()
        flock(lock_fd, fcntl.LOCK_UN)
        return dict(schema='s3-local-chain-writer-probe/1', home=str(home),
            lock_device=after.st_dev, lock_inode=after.st_ino,
            started_monotonic_ns=started, finished_monotonic_ns=finished,
            writer_lock_reacquired=True, continuous_exclusion_verified=False,
            cleanup_complete_verified=False)
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception:
        raise ValueError(ERROR) from None
    finally:
        try:
            if lock_fd is not None:
                os.close(lock_fd)
        finally:
            if root_fd is not None:
                os.close(root_fd)
