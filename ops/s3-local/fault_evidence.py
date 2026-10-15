"""Single-use, private response-loss evidence. No service or automatic retry.

Two JSONL records: reservation and final observation. Missing/torn final record
means unknown, including SIGKILL/power loss. fsync is not a host durability claim.
Caller owns publishing evidence; errors never delete or replace partial records.
"""
import json
import os
import re
import stat
from preflight import checked_root


def _report(fault):
    value = fault.report()
    flags = ('used', 'upstream_attempted', 'upstream_returned', 'response_discarded')
    expected = set(flags) | {'schema', 'body_sha256', 'chain_effect_verified', 'DEV', 'durable_ack'}
    if (type(value) is not dict or set(value) != expected or
        value['schema'] != 's3-local-response-loss/1' or
        type(value['body_sha256']) is not str or
        re.fullmatch('[0-9a-f]{64}', value['body_sha256']) is None or
        any(type(value[k]) is not bool for k in flags) or
        value['chain_effect_verified'] is not False or value['DEV'] != 'NOT_RUN' or
        value['durable_ack'] is not False or
        value['response_discarded'] != value['upstream_returned'] or
        (value['upstream_returned'] and not value['upstream_attempted']) or
        (value['upstream_attempted'] and not value['used'])):
        raise ValueError('FAULT_REPORT')
    return dict(value)


def record(root, fault, action):
    """Reserve/fsync before action; append bounded final state even on BaseException."""
    initial = _report(fault)
    if initial['used'] or not callable(action):
        raise ValueError('FAULT_EVIDENCE_INPUT')
    root = checked_root(root)
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        identity = os.fstat(directory)
        if identity.st_uid != os.getuid() or stat.S_IMODE(identity.st_mode) != 0o700:
            raise ValueError('FAULT_EVIDENCE_ROOT')
        name = 'response-loss.jsonl'
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=directory)
        with os.fdopen(fd, 'wb') as output:
            os.fchmod(output.fileno(), 0o600)
            file_id = os.fstat(output.fileno())
            def check():
                current = os.stat(root, follow_symlinks=False)
                named = os.stat(name, dir_fd=directory, follow_symlinks=False)
                if (current.st_dev, current.st_ino, current.st_mode, current.st_uid) != (identity.st_dev, identity.st_ino, identity.st_mode, identity.st_uid):
                    raise ValueError('FAULT_EVIDENCE_ROOT_CHANGED')
                if (named.st_dev, named.st_ino, named.st_nlink, named.st_mode, named.st_uid) != (file_id.st_dev, file_id.st_ino, 1, file_id.st_mode, file_id.st_uid):
                    raise ValueError('FAULT_EVIDENCE_FILE_CHANGED')
            def append(phase, state):
                check()
                raw = json.dumps(dict(phase=phase, report=state), sort_keys=True, separators=(',', ':')).encode() + b'\n'
                output.write(raw)
                output.flush()
                os.fsync(output.fileno())
                os.fsync(directory)
                check()
            append('reserved', initial)
            phase = 'interrupted_or_failed'
            try:
                result = action()
                phase = 'returned'
                return result
            finally:
                final = _report(fault)
                if final['body_sha256'] != initial['body_sha256']:
                    raise ValueError('FAULT_REPORT_CHANGED')
                append(phase, final)
    finally:
        os.close(directory)
