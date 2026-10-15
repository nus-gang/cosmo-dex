#!/usr/bin/env python3
"""Inspect F14 record integrity without certifying effects or Apply success."""
import base64
import hashlib
import json
import re
import sys
from storage_fault_report import CAP, _object, _inspect_file
FIELDS = set("schema phase command_sha256 command_base64 effect selected_phase occurrence prepare_visits replay_visits injected command_success_verified durable_ack DEV".split())

ERROR = 'CORRECTION_FAULT_REPORT_INVALID'


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
            if type(r) is not dict or set(r) != FIELDS:
                raise ValueError()
            if (r['schema'] != 's3-local-correction-fault/1' or r['effect'] != 'F14_PREPARE_ERROR'
                    or r['selected_phase'] != 'Prepare' or r['command_success_verified'] is not False
                    or r['durable_ack'] is not False or r['DEV'] != 'NOT_RUN'):
                raise ValueError()
            data = base64.b64decode(r['command_base64'], validate=True)
            if not 0 < len(data) <= 16384 or base64.b64encode(data).decode() != r['command_base64']:
                raise ValueError()
            if hashlib.sha256(data).hexdigest() != r['command_sha256'] or r['command_sha256'] != expected_command_sha256:
                raise ValueError()
            command = json.loads(data, object_pairs_hook=_object)
            if (type(command) is not dict or command.get('effect') != 'F14_PREPARE_ERROR'
                    or command.get('phase') != 'Prepare' or command.get('occurrence') != str(r['occurrence'])):
                raise ValueError()
            if type(r['occurrence']) is not int or not 1 <= r['occurrence'] <= 1024:
                raise ValueError()
            if (any(type(r[k]) is not int for k in ('prepare_visits', 'replay_visits'))
                    or min(r['prepare_visits'], r['replay_visits']) < 0
                    or r['prepare_visits'] + r['replay_visits'] > 65536
                    or r['prepare_visits'] > r['occurrence'] or type(r['injected']) is not bool
                    or r['injected'] != (r['prepare_visits'] == r['occurrence'])):
                raise ValueError()
            records.append(r)
        first = records[0]
        if first['phase'] != 'reserved' or first['prepare_visits'] != 0 or first['replay_visits'] != 0 or first['injected']:
            raise ValueError()
        phase = None
        if len(records) == 2:
            last = records[1]
            if last['phase'] not in ('scope_returned', 'scope_error', 'panic'):
                raise ValueError()
            if any(first[k] != last[k] for k in set(first) - {'phase', 'prepare_visits', 'replay_visits', 'injected'}):
                raise ValueError()
            phase = last['phase']
        return dict(schema='s3-local-correction-fault-inspection/1', command_sha256=expected_command_sha256,
                    file_sha256=hashlib.sha256(raw).hexdigest(), final_phase=phase, partial_tail=bool(tail),
                    observation=('RECORDED_INJECTION' if records[-1]['injected'] else 'RECORDED_NOT_REACHED') if phase else 'UNKNOWN',
                    injection_verified=False, command_success_verified=False, authenticity_verified=False,
                    fsync_verified=False, replay_verified=False, durable_ack=False, DEV='NOT_RUN')
    except Exception:
        raise ValueError(ERROR) from None


def inspect(root, expected_command_sha256):
    try:
        return _inspect_file(root, expected_command_sha256, 'correction-fault.jsonl', inspect_bytes)
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
