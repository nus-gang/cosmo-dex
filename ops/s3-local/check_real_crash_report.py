"""Test bridge for real Rust crash records; does not alter evidence."""
import hashlib
import json
from pathlib import Path
import sys

from storage_crash_report import inspect, inspect_bytes, ERROR


def check(root, digest, phase):
    path = Path(root) / 'storage-crash.jsonl'
    before = path.stat()
    raw = path.read_bytes()
    result = inspect(root, digest)
    assert result['final_phase'] == (None if phase == 'UNKNOWN' else phase)
    assert result['observation'] == ('UNKNOWN' if phase == 'UNKNOWN' else 'RECORDED_NOT_REACHED')
    assert result['file_sha256'] == hashlib.sha256(raw).hexdigest()
    for field in ('crash_verified', 'command_success_verified', 'authenticity_verified',
                  'fsync_verified', 'replay_verified', 'durable_ack'):
        assert result[field] is False
    assert result['DEV'] == 'NOT_RUN'
    try:
        inspect(root, '0' * 64)
    except ValueError as error:
        assert str(error) == ERROR
    else:
        raise AssertionError('wrong command accepted')
    first = raw.split(b'\n')[0] + b'\n'
    for partial in (first, first + b'{"schema":'):
        unknown = inspect_bytes(partial, digest)
        assert unknown['observation'] == 'UNKNOWN'
        assert unknown['final_phase'] is None
    forged = json.loads(first)
    forged.update(phase='scope_returned', matching_visits=forged['occurrence'],
                  visits=forged['occurrence'], injected=True)
    try:
        inspect_bytes(first + json.dumps(forged).encode() + b'\n', digest)
    except ValueError as error:
        assert str(error) == ERROR
    else:
        raise AssertionError('returned crash accepted')
    assert path.read_bytes() == raw
    after = path.stat()
    for field in ('st_dev', 'st_ino', 'st_mode', 'st_size', 'st_mtime_ns', 'st_ctime_ns'):
        assert getattr(before, field) == getattr(after, field)


if __name__ == '__main__':
    check(*sys.argv[1:])
    print('REAL_CRASH_WRITER_REPORT_PASS')
