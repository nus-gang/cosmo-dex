#!/usr/bin/env python3
"""Read F09 Receipt->Apply provenance without treating it as Apply evidence."""
import base64
import hashlib
import json
import re
import sys

from storage_fault_report import CAP, _inspect_file, _object

ERROR = 'RECEIPT_APPLY_REPORT_INVALID'
FIELDS = set('schema phase boundary_base64 boundary_sha256 receipt_sha256 batch_id disposition apply_called crash_verified durable_ack DEV'.split())
BOUNDARY = set('schema boundary batch_id disposition receipt_sha256 before_commit after_commit apply_called reusable_permit'.split())
COMMIT = {'command_seq', 'record_hash', 'end_offset'}
HEX = re.compile('[0-9a-f]{64}')
DECIMAL = re.compile('0|[1-9][0-9]{0,19}')


def _commit(value):
    if type(value) is not dict or set(value) != COMMIT:
        raise ValueError()
    if not HEX.fullmatch(value['record_hash']):
        raise ValueError()
    for key in ('command_seq', 'end_offset'):
        if type(value[key]) is not str or not DECIMAL.fullmatch(value[key]):
            raise ValueError()
    return int(value['command_seq']), int(value['end_offset'])


def inspect_bytes(raw, expected_boundary_sha256):
    try:
        if type(raw) is not bytes or not 0 < len(raw) <= CAP or not HEX.fullmatch(expected_boundary_sha256):
            raise ValueError()
        *complete, tail = raw.split(b'\n')
        if not 1 <= len(complete) <= 2 or (len(complete) == 2 and tail):
            raise ValueError()
        records = []
        for line in complete:
            record = json.loads(line, object_pairs_hook=_object)
            if (type(record) is not dict or set(record) != FIELDS
                    or record['schema'] != 's3-local-receipt-apply/1'
                    or record['DEV'] != 'NOT_RUN'):
                raise ValueError()
            if any(record[key] is not False for key in ('apply_called', 'crash_verified', 'durable_ack')):
                raise ValueError()
            data = base64.b64decode(record['boundary_base64'], validate=True)
            if not 0 < len(data) <= 16384 or base64.b64encode(data).decode() != record['boundary_base64']:
                raise ValueError()
            digest = hashlib.sha256(data).hexdigest()
            if digest != record['boundary_sha256'] or digest != expected_boundary_sha256:
                raise ValueError()
            boundary = json.loads(data, object_pairs_hook=_object)
            if (type(boundary) is not dict or set(boundary) != BOUNDARY
                    or boundary['schema'] != 'sre-receipt-apply-boundary/1'
                    or boundary['boundary'] != 'F09'
                    or boundary['apply_called'] is not False
                    or boundary['reusable_permit'] is not False):
                raise ValueError()
            if (type(boundary['batch_id']) is not str or not HEX.fullmatch(boundary['batch_id'])
                    or type(boundary['receipt_sha256']) is not str or not HEX.fullmatch(boundary['receipt_sha256'])
                    or boundary['disposition'] not in ('COMMITTED', 'VOID')):
                raise ValueError()
            before_seq, before_offset = _commit(boundary['before_commit'])
            after_seq, after_offset = _commit(boundary['after_commit'])
            if after_seq <= before_seq or after_offset <= before_offset:
                raise ValueError()
            if any(record[key] != boundary[key] for key in ('batch_id', 'disposition', 'receipt_sha256')):
                raise ValueError()
            records.append(record)
        first = records[0]
        if first['phase'] != 'reserved':
            raise ValueError()
        final_phase = None
        if len(records) == 2:
            last = records[1]
            if (last['phase'] not in ('boundary_returned', 'boundary_error', 'panic')
                    or any(first[key] != last[key] for key in FIELDS - {'phase'})):
                raise ValueError()
            final_phase = last['phase']
        return dict(schema='s3-local-receipt-apply-inspection/1',
                    boundary_sha256=expected_boundary_sha256,
                    file_sha256=hashlib.sha256(raw).hexdigest(),
                    final_phase=final_phase, partial_tail=bool(tail),
                    observation='RECORDED_BOUNDARY_RETURN' if final_phase else 'UNKNOWN',
                    apply_verified=False, crash_verified=False,
                    receipt_durability_verified=False, authenticity_verified=False,
                    fsync_verified=False, replay_verified=False,
                    reusable_permit=False, durable_ack=False, DEV='NOT_RUN')
    except Exception:
        raise ValueError(ERROR) from None


def inspect(root, expected_boundary_sha256):
    try:
        return _inspect_file(root, expected_boundary_sha256,
                             'receipt-apply.jsonl', inspect_bytes)
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
