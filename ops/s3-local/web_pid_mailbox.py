"""Single-process web PID evidence. No child-tree or cleanup certification.

Reuses the private, nonce-bound, no-replace/fsync mailbox storage boundary.
Records before any listener creation. Preserve the directory even on failure.
"""
import json
import os
from pid_mailbox import Mailbox, _root, _read, _write, _nonce, ERROR


def _report(nonce, pid):
    return json.dumps({'schema': 's3-local-web-pid/1', 'nonce': nonce,
                       'web_pid': pid}, sort_keys=True, separators=(',', ':')).encode()


def reporter(path):
    try:
        root = _root(path)
        identity = root.stat()
        nonce = _nonce(_read(root, 'challenge'))
    except Exception:
        raise ValueError(ERROR) from None
    used = False

    def record():
        nonlocal used
        if used:
            raise ValueError(ERROR)
        used = True
        try:
            m = _root(root).stat()
            if ((m.st_dev, m.st_ino) != (identity.st_dev, identity.st_ino) or
                    _nonce(_read(root, 'challenge')) != nonce):
                raise ValueError()
            pid = os.getpid()
            if not 2 <= pid <= 2147483647:
                raise ValueError()
            _write(root, 'pids.json', _report(nonce, pid))
        except Exception:
            raise ValueError(ERROR) from None
    return record


class WebMailbox(Mailbox):
    def collect(self):
        if self.used:
            raise ValueError(ERROR)
        self.used = True
        try:
            m = _root(self.root).stat()
            if ((m.st_dev, m.st_ino) != (self.identity.st_dev, self.identity.st_ino) or
                    _nonce(_read(self.root, 'challenge')) != self.nonce):
                raise ValueError()
            raw = _read(self.root, 'pids.json')
            report = json.loads(raw)
            pid = report['web_pid']
            if (type(pid) is not int or not 2 <= pid <= 2147483647 or
                    raw != _report(self.nonce, pid)):
                raise ValueError()
            return (pid,)
        except Exception:
            raise ValueError(ERROR) from None
