#!/usr/bin/env python3
"""Read F06/F07 provenance without claiming a socket, result, or asset effect."""
import base64
import hashlib
import json
import re
import sys
from storage_fault_report import CAP, _object, _inspect_file

ERROR = 'RPC_RESPONSE_REPORT_INVALID'
FIELDS = set('schema phase boundary_base64 boundary_sha256 socket_verified response_complete attempt_resolved asset_effect_verified crash_verified durable_ack DEV'.split())
BOUNDARY = set('schema boundary state tx_hash broadcast_count raw_len stored_intent_sha256 request_sha256 headers_sha256 partial_json_sha256 partial_json_len transport_callback_called socket_verified response_complete attempt_resolved reusable_permit'.split())
HEX = re.compile('[0-9a-f]{64}')


def inspect_bytes(raw, expected_boundary_sha256):
    try:
        if type(raw) is not bytes or not 0 < len(raw) <= CAP or not HEX.fullmatch(expected_boundary_sha256):
            raise ValueError()
        *complete, tail = raw.split(b'\n')
        if not 1 <= len(complete) <= 2 or (len(complete) == 2 and tail):
            raise ValueError()
        records = []
        for line in complete:
            row = json.loads(line, object_pairs_hook=_object)
            if type(row) is not dict or set(row) != FIELDS or row['schema'] != 's3-local-rpc-response/1' or row['DEV'] != 'NOT_RUN':
                raise ValueError()
            if any(row[key] is not False for key in FIELDS - {'schema', 'phase', 'boundary_base64', 'boundary_sha256', 'DEV'}):
                raise ValueError()
            data = base64.b64decode(row['boundary_base64'], validate=True)
            if not 0 < len(data) <= 16384 or base64.b64encode(data).decode() != row['boundary_base64']:
                raise ValueError()
            if hashlib.sha256(data).hexdigest() != row['boundary_sha256'] or row['boundary_sha256'] != expected_boundary_sha256:
                raise ValueError()
            boundary = json.loads(data, object_pairs_hook=_object)
            if type(boundary) is not dict or set(boundary) != BOUNDARY or boundary['schema'] != 'sre-rpc-response-boundary/1':
                raise ValueError()
            if boundary['boundary'] not in ('F06', 'F07') or boundary['state'] != 'SUBMISSION_UNKNOWN':
                raise ValueError()
            if boundary['transport_callback_called'] is not True or any(boundary[key] is not False for key in ('socket_verified','response_complete','attempt_resolved','reusable_permit')):
                raise ValueError()
            if any(type(boundary[key]) is not str or not HEX.fullmatch(boundary[key]) for key in ('tx_hash','stored_intent_sha256','request_sha256')):
                raise ValueError()
            if boundary['request_sha256'] != boundary['tx_hash'] or boundary['broadcast_count'] not in ('1','2','3'):
                raise ValueError()
            if type(boundary['raw_len']) is not str or not re.fullmatch('[1-9][0-9]{0,5}', boundary['raw_len']) or int(boundary['raw_len']) > 139264:
                raise ValueError()
            if type(boundary['partial_json_len']) is not str or not re.fullmatch('0|[1-9][0-9]{0,4}', boundary['partial_json_len']):
                raise ValueError()
            if boundary['boundary'] == 'F06':
                if boundary['headers_sha256'] is not None or boundary['partial_json_sha256'] is not None or boundary['partial_json_len'] != '0':
                    raise ValueError()
            elif any(type(boundary[key]) is not str or not HEX.fullmatch(boundary[key]) for key in ('headers_sha256','partial_json_sha256')) or not 1 <= int(boundary['partial_json_len']) <= 65536:
                raise ValueError()
            records.append(row)
        if records[0]['phase'] != 'reserved':
            raise ValueError()
        phase = None
        if len(records) == 2:
            phase = records[1]['phase']
            if phase not in ('boundary_returned','boundary_error','panic') or any(records[0][key] != records[1][key] for key in FIELDS - {'phase'}):
                raise ValueError()
        return dict(schema='s3-local-rpc-response-inspection/1', boundary_sha256=expected_boundary_sha256,
                    file_sha256=hashlib.sha256(raw).hexdigest(), final_phase=phase, partial_tail=bool(tail),
                    observation='RECORDED_RESPONSE_BOUNDARY' if phase else 'UNKNOWN', socket_verified=False,
                    response_complete=False, attempt_resolved=False, asset_effect_verified=False,
                    crash_verified=False, authenticity_verified=False, fsync_verified=False,
                    replay_verified=False, reusable_permit=False, durable_ack=False, DEV='NOT_RUN')
    except Exception:
        raise ValueError(ERROR) from None


def inspect(root, expected_boundary_sha256):
    try:
        return _inspect_file(root, expected_boundary_sha256, 'rpc-response.jsonl', inspect_bytes)
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
