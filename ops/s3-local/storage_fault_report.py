#!/usr/bin/env python3
"""Read v2/v3 fault evidence without effects. Integrity is not authenticity/durability."""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
from storage_fault_ready import POINTS

CAP = 65536
ERROR = 'STORAGE_FAULT_REPORT_INVALID'
FIELDS = set('schema phase command_sha256 command_base64 point occurrence visits matching_visits injected durable_ack DEV'.split())


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(ERROR)
        result[key] = value
    return result


def inspect_bytes(raw, expected_command_sha256):
    try:
        if type(raw) is not bytes or not 0 < len(raw) <= CAP or not re.fullmatch('[0-9a-f]{64}', expected_command_sha256):
            raise ValueError()
        lines = raw.split(b'\n')
        complete = lines[:-1]
        tail = lines[-1]
        if not 1 <= len(complete) <= 2 or (len(complete) == 2 and tail):
            raise ValueError()
        records = []
        for line in complete:
            r = json.loads(line.decode('utf-8'), object_pairs_hook=_object)
            if type(r) is not dict:
                raise ValueError()
            v3 = r.get('schema') == 's3-local-storage-fault/3'
            if set(r) != FIELDS | ({'io_fault'} if v3 else set()):
                raise ValueError()
            if r['schema'] not in ('s3-local-storage-fault/2', 's3-local-storage-fault/3') or r['durable_ack'] is not False or r['DEV'] != 'NOT_RUN':
                raise ValueError()
            data = base64.b64decode(r['command_base64'], validate=True)
            if not 0 < len(data) <= 16384 or base64.b64encode(data).decode() != r['command_base64']:
                raise ValueError()
            if hashlib.sha256(data).hexdigest() != r['command_sha256'] or r['command_sha256'] != expected_command_sha256:
                raise ValueError()
            if v3:
                if r['io_fault'] not in ('ENOSPC', 'EDQUOT', 'EIO'):
                    raise ValueError()
                command = json.loads(data, object_pairs_hook=_object)
                if (type(command) is not dict or command.get('io_fault') != r['io_fault']
                        or command.get('point') != r['point']
                        or command.get('occurrence') != str(r['occurrence'])):
                    raise ValueError()
            if r['point'] not in POINTS or type(r['occurrence']) is not int or not 1 <= r['occurrence'] <= 1024:
                raise ValueError()
            if any(type(r[k]) is not int for k in ('visits', 'matching_visits')) or not 0 <= r['matching_visits'] <= r['visits'] <= 65536:
                raise ValueError()
            if type(r['injected']) is not bool or r['matching_visits'] > r['occurrence'] or r['injected'] != (r['matching_visits'] == r['occurrence']):
                raise ValueError()
            records.append(r)
        first = records[0]
        if first['phase'] != 'reserved' or first['visits'] != 0 or first['matching_visits'] != 0 or first['injected']:
            raise ValueError()
        observation = 'UNKNOWN'
        phase = None
        if len(records) == 2:
            last = records[1]
            if last['phase'] not in ('scope_returned', 'scope_error', 'panic'):
                raise ValueError()
            for key in set(first) - {'phase', 'visits', 'matching_visits', 'injected'}:
                if first[key] != last[key]:
                    raise ValueError()
            phase = last['phase']
            observation = 'RECORDED_INJECTION' if last['injected'] else 'RECORDED_NOT_REACHED'
        return dict(schema='s3-local-storage-fault-inspection/1', command_sha256=expected_command_sha256,
                    file_sha256=hashlib.sha256(raw).hexdigest(), observation=observation,
                    final_phase=phase, partial_tail=bool(tail), io_fault=first.get('io_fault', 'GENERIC'), seal_success_verified=False,
                    authenticity_verified=False, fsync_verified=False, replay_verified=False,
                    durable_ack=False, DEV='NOT_RUN')
    except Exception:
        raise ValueError(ERROR) from None


def inspect(root, expected_command_sha256):
    root_fd = fd = None
    try:
        root = Path(root)
        if not root.is_absolute() or root.resolve(strict=True) != root:
            raise ValueError()
        root_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        rm = os.fstat(root_fd)
        if stat.S_IMODE(rm.st_mode) != 0o700 or rm.st_uid != os.getuid():
            raise ValueError()
        fd = os.open('storage-fault.jsonl', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=root_fd)
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or stat.S_IMODE(before.st_mode) != 0o600 or before.st_uid != os.getuid() or before.st_nlink != 1 or before.st_size > CAP:
            raise ValueError()
        raw = b''
        while len(raw) <= CAP:
            part = os.read(fd, min(8192, CAP + 1 - len(raw)))
            if not part:
                break
            raw += part
        identity = lambda s: (s.st_dev, s.st_ino, s.st_mode, s.st_uid, s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
        if identity(before) != identity(os.fstat(fd)) or identity(before) != identity(os.stat('storage-fault.jsonl', dir_fd=root_fd, follow_symlinks=False)) or identity(rm) != identity(os.stat(root, follow_symlinks=False)) or root.resolve(strict=True) != root:
            raise ValueError()
        return inspect_bytes(raw, expected_command_sha256)
    except Exception:
        raise ValueError(ERROR) from None
    finally:
        try:
            if fd is not None:
                os.close(fd)
        finally:
            if root_fd is not None:
                os.close(root_fd)


def main(argv=None):
    try:
        args = sys.argv[1:] if argv is None else argv
        if len(args) != 3 or args[0] != 'inspect':
            raise ValueError()
        result = inspect(args[1], args[2])
        print(json.dumps(result, sort_keys=True))
        return 0
    except Exception:
        print(ERROR, file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
