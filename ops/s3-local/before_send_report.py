#!/usr/bin/env python3
"""Read F05 provenance; record integrity does not prove transmission or crash."""
import base64
import hashlib
import json
import re
import sys
from storage_fault_report import CAP, _object, _inspect_file

ERROR = 'BEFORE_SEND_REPORT_INVALID'
FIELDS = set('schema phase boundary_base64 boundary_sha256 transport_called crash_verified command_success_verified durable_ack DEV'.split())
BOUNDARY = set('schema boundary state tx_hash broadcast_count raw_len stored_intent_sha256 transport_called crash_verified reusable_permit'.split())


def inspect_bytes(raw, expected_boundary_sha256):
    try:
        if type(raw) is not bytes or not 0 < len(raw) <= CAP or not re.fullmatch('[0-9a-f]{64}', expected_boundary_sha256):
            raise ValueError()
        *complete, tail = raw.split(b'\n')
        if not 1 <= len(complete) <= 2 or (len(complete) == 2 and tail):
            raise ValueError()
        records = []
        for line in complete:
            r = json.loads(line, object_pairs_hook=_object)
            if type(r) is not dict or set(r) != FIELDS or r['schema'] != 's3-local-before-send/1' or r['DEV'] != 'NOT_RUN':
                raise ValueError()
            if any(r[k] is not False for k in ('transport_called','crash_verified','command_success_verified','durable_ack')):
                raise ValueError()
            data = base64.b64decode(r['boundary_base64'], validate=True)
            if not 0 < len(data) <= 16384 or base64.b64encode(data).decode() != r['boundary_base64']:
                raise ValueError()
            if hashlib.sha256(data).hexdigest() != r['boundary_sha256'] or r['boundary_sha256'] != expected_boundary_sha256:
                raise ValueError()
            b = json.loads(data, object_pairs_hook=_object)
            if type(b) is not dict or set(b) != BOUNDARY or b['schema'] != 'sre-before-send-boundary/1' or b['boundary'] != 'F05' or b['state'] != 'SUBMISSION_UNKNOWN':
                raise ValueError()
            if any(b[k] is not False for k in ('transport_called','crash_verified','reusable_permit')):
                raise ValueError()
            if any(type(b[k]) is not str or not re.fullmatch('[0-9a-f]{64}', b[k]) for k in ('tx_hash','stored_intent_sha256')):
                raise ValueError()
            if b['broadcast_count'] not in ('1','2','3') or type(b['raw_len']) is not str or not re.fullmatch('[1-9][0-9]{0,5}', b['raw_len']) or int(b['raw_len']) > 139264:
                raise ValueError()
            records.append(r)
        if records[0]['phase'] != 'reserved':
            raise ValueError()
        phase = None
        if len(records) == 2:
            phase = records[1]['phase']
            if phase not in ('boundary_returned','boundary_error','panic') or any(records[0][k] != records[1][k] for k in FIELDS - {'phase'}):
                raise ValueError()
        return dict(schema='s3-local-before-send-inspection/1', boundary_sha256=expected_boundary_sha256,
                    file_sha256=hashlib.sha256(raw).hexdigest(), final_phase=phase, partial_tail=bool(tail),
                    observation='RECORDED_BOUNDARY_RETURN' if phase else 'UNKNOWN',
                    crash_verified=False, transport_verified=False, command_success_verified=False,
                    authenticity_verified=False, fsync_verified=False, replay_verified=False,
                    reusable_permit=False, durable_ack=False, DEV='NOT_RUN')
    except Exception:
        raise ValueError(ERROR) from None


def inspect(root, expected_boundary_sha256):
    try:
        return _inspect_file(root, expected_boundary_sha256, 'before-send.jsonl', inspect_bytes)
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
