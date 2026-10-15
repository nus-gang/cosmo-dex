"""Test bridge: inspect actual Rust writer output; never modify the evidence."""
import hashlib
from pathlib import Path
import sys

from storage_fault_report import inspect, inspect_bytes


def check(root, digest):
    path = Path(root) / 'storage-fault.jsonl'
    before = path.stat()
    raw = path.read_bytes()
    result = inspect(root, digest)
    assert result['observation'] == 'RECORDED_INJECTION'
    assert result['final_phase'] == 'scope_returned'
    assert result['file_sha256'] == hashlib.sha256(raw).hexdigest()
    for field in ('seal_success_verified', 'authenticity_verified',
                  'fsync_verified', 'replay_verified', 'durable_ack'):
        assert result[field] is False
    assert result['DEV'] == 'NOT_RUN'
    try:
        inspect(root, '0' * 64)
    except ValueError as error:
        assert str(error) == 'STORAGE_FAULT_REPORT_INVALID'
    else:
        raise AssertionError('wrong command accepted')
    first, final, empty = raw.split(b'\n')
    assert empty == b''
    for partial in (first + b'\n', first + b'\n' + final[:len(final)//2]):
        unknown = inspect_bytes(partial, digest)
        assert unknown['observation'] == 'UNKNOWN'
        assert unknown['final_phase'] is None
    assert path.read_bytes() == raw
    after = path.stat()
    for field in ('st_dev', 'st_ino', 'st_mode', 'st_size', 'st_mtime_ns', 'st_ctime_ns'):
        assert getattr(before, field) == getattr(after, field)


if __name__ == '__main__':
    check(sys.argv[1], sys.argv[2])
    print('REAL_WRITER_REPORT_PASS')
