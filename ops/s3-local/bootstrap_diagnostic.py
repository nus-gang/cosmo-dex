"""Private one-attempt bootstrap journal. Text is a closed vocabulary, not stderr.

The wire file is untrusted socket evidence, never the Snapshot entity. A partial
journal (including an fsync failure) is evidence of uncertainty, not permission.
"""
from contextlib import contextmanager
import hashlib
import json
import os
import stat
import time
from preflight import checked_root

WIRE_LIMIT = 16_777_216 + 262_144
EVENT_LIMIT = 1024 * 1024
STAGES = frozenset('begin initial_audit input_capture reserve_entity reserve_sync '
    'fetch_audit fetch_capture fetch_binary checker prefetch_audit child_spawn child_exit '
    'child_capture request_start connected sent received entity_complete '
    'postfetch_audit persist_entity create_audit create_stage create_run complete failed'.split())
CODES = frozenset('OK REJECTED IO INTERRUPTED STOPPED FETCH_TIMEOUT FETCH_INPUT_CLOSED '
    'FETCH_OUTPUT_LIMIT FETCH_REJECTED FETCH_STOPPED FETCH_POSTCHECK_REJECTED '
    'QUERY_POLICY QUERY_ENCODING QUERY_CONNECT QUERY_IO QUERY_EOF QUERY_SIZE '
    'QUERY_HEADER QUERY_TIMEOUT QUERY_LENGTH QUERY_STATUS QUERY_FRAMING QUERY_EMPTY '
    'QUERY_CONTENT_TYPE QUERY_CHUNKS QUERY_CHUNK QUERY_TRAILING QUERY_EVIDENCE'.split())


def error_code(error):
    if isinstance(error, (KeyboardInterrupt, SystemExit)): return 'INTERRUPTED'
    if isinstance(error, OSError): return 'IO'
    return str(error) if isinstance(error, ValueError) and str(error) in CODES else 'REJECTED'


class Diagnostic:
    def __init__(self, root):
        self.root = checked_root(root)
        self.directory = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        self.files = {}; self.sizes = {}; self.identities = {}; self.phase = 'begin'
        self.last = 0
        try:
            self.identity = os.fstat(self.directory)
            if self.identity.st_uid != os.getuid() or stat.S_IMODE(self.identity.st_mode) != 0o700:
                raise ValueError('DIAGNOSTIC_ROOT')
            for name in ('bootstrap-diagnostic.jsonl', 'snapshot-fetch.wire'):
                fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=self.directory)
                self.files[name] = fd; self.sizes[name] = 0
                os.fchmod(fd, 0o600); self.identities[name] = os.fstat(fd)
            self.emit('begin')
            os.fsync(self.directory)
        except BaseException:
            self.close(); raise

    def verify(self):
        m = os.stat(self.root, follow_symlinks=False)
        if (m.st_dev, m.st_ino, m.st_mode) != (self.identity.st_dev, self.identity.st_ino, self.identity.st_mode):
            raise ValueError('DIAGNOSTIC_ROOT_CHANGED')
        for name, expected in self.identities.items():
            m = os.stat(name, dir_fd=self.directory, follow_symlinks=False)
            if (m.st_dev, m.st_ino, m.st_nlink, m.st_uid, stat.S_IMODE(m.st_mode)) != (
                    expected.st_dev, expected.st_ino, 1, os.getuid(), 0o600):
                raise ValueError('DIAGNOSTIC_FILE_CHANGED')

    def _write(self, name, raw, cap):
        self.verify()
        if self.sizes[name] + len(raw) > cap: raise ValueError('DIAGNOSTIC_LIMIT')
        view = memoryview(raw)
        while view:
            n = os.write(self.files[name], view)
            if n <= 0: raise OSError('DIAGNOSTIC_IO')
            self.sizes[name] += n; view = view[n:]
        os.fsync(self.files[name])

    def emit(self, stage, *, code='OK', **fields):
        if stage not in STAGES or code not in CODES: raise ValueError('DIAGNOSTIC_FIELDS')
        for k, v in fields.items():
            if k in ('capture_sha256', 'binary_sha256', 'checker_sha256', 'entity_sha256'):
                if not isinstance(v, str) or len(v) != 64 or any(c not in '0123456789abcdef' for c in v):
                    raise ValueError('DIAGNOSTIC_FIELDS')
            elif k in ('pid', 'sent_bytes', 'received_bytes', 'entity_bytes', 'child_elapsed_ns', 'exit_code'):
                if type(v) is not int or not (-255 if k=='exit_code' else 0) <= v <= 2**63-1: raise ValueError('DIAGNOSTIC_FIELDS')
            else: raise ValueError('DIAGNOSTIC_FIELDS')
        now = time.monotonic_ns()
        if now < self.last: raise ValueError('DIAGNOSTIC_CLOCK')
        self.last = now
        if stage not in ('failed','child_exit'): self.phase = stage
        value = dict(schema='s3-local-bootstrap-diagnostic/1', stage=stage,
                     phase=self.phase, code=code, monotonic_ns=now, **fields)
        self._write('bootstrap-diagnostic.jsonl', (json.dumps(value, sort_keys=True)+'\n').encode(), EVENT_LIMIT)

    def wire(self, raw):
        self._write('snapshot-fetch.wire', raw, WIRE_LIMIT)

    def close(self):
        for fd in self.files.values(): os.close(fd)
        self.files.clear()
        if self.directory is not None:
            os.close(self.directory); self.directory = None


@contextmanager
def diagnostic(root):
    trace = Diagnostic(root)
    try:
        yield trace
    except BaseException as error:
        # Do not replace the primary error if storage itself has failed. Bytes
        # already written stay in place; missing terminal event denies success.
        try: trace.emit('failed', code=error_code(error))
        except BaseException: pass
        raise
    finally:
        trace.close()
