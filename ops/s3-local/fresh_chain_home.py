"""Publish already-validated local chain inputs into a NEW private home.

No key generation, genesis semantics, approval, database, listener or cleanup.
Caller supplies fresh private keys; never use public fixture keys for a service.
Partial homes are retained on any failure and cannot be retried in place.
"""
import os
from pathlib import Path
import stat


class HomeError(ValueError):
    pass


FILES = ('genesis.json', 'priv_validator_key.json', 'node_key.json',
         'priv_validator_state.json')


def publish(home, *, guard, files):
    home = Path(home)
    if (not home.is_absolute() or str(home) != os.path.normpath(str(home))
            or home.parent.resolve(strict=True) != home.parent):
        raise HomeError('CANONICAL_PARENT_REQUIRED')
    parent = home.parent.stat()
    if (parent.st_uid != os.getuid() or stat.S_IMODE(parent.st_mode) != 0o700):
        raise HomeError('PRIVATE_PARENT_REQUIRED')
    if type(files) is not dict or set(files) != set(FILES):
        raise HomeError('EXACT_FILES_REQUIRED')
    values = {'guard.dev.json': guard, **files}
    if any(type(v) is not bytes or not 0 < len(v) <= 1 << 20 for v in values.values()):
        raise HomeError('BOUNDED_BYTES_REQUIRED')
    # Copy before filesystem effects. Input bytes are immutable.
    values = dict(values)
    parent_fd = os.open(home.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    root_fd = None
    try:
        opened = os.fstat(parent_fd)
        if (opened.st_dev, opened.st_ino) != (parent.st_dev, parent.st_ino):
            raise HomeError('PARENT_CHANGED')
        os.mkdir(home.name, 0o700, dir_fd=parent_fd)  # no replacement, even empty home
        os.fsync(parent_fd)
        root_fd = os.open(home.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                          dir_fd=parent_fd)
        def write(directory, name, raw):
            fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o600, dir_fd=directory)
            try:
                view = memoryview(raw)
                while view:
                    n = os.write(fd, view)
                    if n <= 0:
                        raise HomeError('SHORT_WRITE')
                    view = view[n:]
                os.fsync(fd)
            finally:
                os.close(fd)
        # Guard publication finishes before config/data or key files exist.
        write(root_fd, 'guard.dev.json', values['guard.dev.json'])
        os.fsync(root_fd)
        for directory, names in (
            ('config', ('genesis.json', 'priv_validator_key.json', 'node_key.json')),
            ('data', ('priv_validator_state.json',)),
        ):
            os.mkdir(directory, 0o700, dir_fd=root_fd)
            fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                         dir_fd=root_fd)
            try:
                for name in names:
                    write(fd, name, values[name])
                os.fsync(fd)
            finally:
                os.close(fd)
        os.fsync(root_fd)
        os.fsync(parent_fd)
        current = home.lstat()
        opened = os.fstat(root_fd)
        if (current.st_dev, current.st_ino) != (opened.st_dev, opened.st_ino):
            raise HomeError('HOME_CHANGED')
        return {'home_created': True, 'semantic_validation_verified': False,
                'runtime_approval_verified': False, 'service_started': False}
    finally:
        if root_fd is not None:
            os.close(root_fd)
        os.close(parent_fd)
