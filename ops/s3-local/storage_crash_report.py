#!/usr/bin/env python3
"""Inspect crash reservation integrity; missing final never proves a crash."""
import base64
import hashlib
import json
import re
import sys
from storage_fault_report import CAP, FIELDS, _object, _inspect_file
from storage_fault_ready import POINTS

ERROR = 'STORAGE_CRASH_REPORT_INVALID'


def inspect_bytes(raw, expected_command_sha256):
    try:
        if type(raw) is not bytes or not 0 < len(raw) <= CAP or not re.fullmatch('[0-9a-f]{64}', expected_command_sha256):
            raise ValueError()
        lines = raw.split(b'\n')
        complete, tail = lines[:-1], lines[-1]
        if not 1 <= len(complete) <= 2 or (len(complete) == 2 and tail):
            raise ValueError()
        records = []
        for line in complete:
            r = json.loads(line, object_pairs_hook=_object)
            if type(r) is not dict or set(r) != FIELDS | {'effect', 'exit_code', 'crash_verified'}:
                raise ValueError()
            if (r['schema'] != 's3-local-storage-crash/1' or r['effect'] != 'IMMEDIATE_EXIT'
                    or type(r['exit_code']) is not int or r['exit_code'] != 86
                    or r['crash_verified'] is not False or r['durable_ack'] is not False or r['DEV'] != 'NOT_RUN'):
                raise ValueError()
            data = base64.b64decode(r['command_base64'], validate=True)
            if not 0 < len(data) <= 16384 or base64.b64encode(data).decode() != r['command_base64']:
                raise ValueError()
            if hashlib.sha256(data).hexdigest() != r['command_sha256'] or r['command_sha256'] != expected_command_sha256:
                raise ValueError()
            command = json.loads(data, object_pairs_hook=_object)
            if (type(command) is not dict or command.get('effect') != 'IMMEDIATE_EXIT'
                    or type(command.get('exit_code')) is not int or command['exit_code'] != 86
                    or command.get('point') != r['point'] or command.get('occurrence') != str(r['occurrence'])):
                raise ValueError()
            if r['point'] not in POINTS or type(r['occurrence']) is not int or not 1 <= r['occurrence'] <= 1024:
                raise ValueError()
            if (any(type(r[k]) is not int for k in ('visits', 'matching_visits'))
                    or not 0 <= r['matching_visits'] <= r['visits'] <= 65536
                    or r['matching_visits'] >= r['occurrence'] or r['injected'] is not False):
                # A hook reaching the crash point cannot return to append final.
                raise ValueError()
            records.append(r)
        first = records[0]
        if first['phase'] != 'reserved' or first['visits'] != 0 or first['matching_visits'] != 0:
            raise ValueError()
        phase = None
        if len(records) == 2:
            last = records[1]
            if last['phase'] not in ('scope_returned', 'scope_error', 'panic'):
                raise ValueError()
            if any(first[k] != last[k] for k in set(first) - {'phase', 'visits', 'matching_visits'}):
                raise ValueError()
            phase = last['phase']
        return dict(schema='s3-local-storage-crash-inspection/1', command_sha256=expected_command_sha256,
                    file_sha256=hashlib.sha256(raw).hexdigest(), final_phase=phase, partial_tail=bool(tail),
                    observation='RECORDED_NOT_REACHED' if phase else 'UNKNOWN',
                    crash_verified=False, command_success_verified=False, authenticity_verified=False,
                    fsync_verified=False, replay_verified=False, durable_ack=False, DEV='NOT_RUN')
    except Exception:
        raise ValueError(ERROR) from None


def inspect(root, expected_command_sha256):
    try:
        return _inspect_file(root, expected_command_sha256, 'storage-crash.jsonl', inspect_bytes)
    except Exception:
        raise ValueError(ERROR) from None


def main(argv=None):
    try:
        args = sys.argv[1:] if argv is None else argv
        if len(args) != 3 or args[0] != 'inspect':
            raise ValueError()
        print(json.dumps(inspect(args[1], args[2]), sort_keys=True))
        return 0
    except Exception:
        print(ERROR, file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
