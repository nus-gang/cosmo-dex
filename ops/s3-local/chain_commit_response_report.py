#!/usr/bin/env python3
"""Read F08 provenance without claiming response delivery or asset effect."""
import base64
import hashlib
import json
import re
import sys

from storage_fault_report import CAP, _inspect_file, _object

ERROR = 'CHAIN_COMMIT_RESPONSE_REPORT_INVALID'
HEX = re.compile('[0-9a-f]{64}')
DECIMAL = re.compile('0|[1-9][0-9]{0,19}')
FIELDS = set('schema phase boundary_base64 boundary_sha256 tx_hash batch_id response_delivered receipt_queried asset_effect_verified crash_verified durable_ack DEV'.split())
BOUNDARY = set('schema boundary tx_hash batch_id height abci_code attempt_state commit response_delivered receipt_queried asset_effect_visible reusable_permit'.split())
COMMIT = {'command_seq', 'record_hash', 'end_offset'}


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
                    or record['schema'] != 's3-local-chain-commit-response/1'
                    or record['DEV'] != 'NOT_RUN'):
                raise ValueError()
            if any(record[key] is not False for key in (
                    'response_delivered', 'receipt_queried', 'asset_effect_verified',
                    'crash_verified', 'durable_ack')):
                raise ValueError()
            data = base64.b64decode(record['boundary_base64'], validate=True)
            if not 0 < len(data) <= 16384 or base64.b64encode(data).decode() != record['boundary_base64']:
                raise ValueError()
            digest = hashlib.sha256(data).hexdigest()
            if digest != record['boundary_sha256'] or digest != expected_boundary_sha256:
                raise ValueError()
            boundary = json.loads(data, object_pairs_hook=_object)
            if (type(boundary) is not dict or set(boundary) != BOUNDARY
                    or boundary['schema'] != 'sre-chain-commit-response-boundary/1'
                    or boundary['boundary'] != 'F08'
                    or boundary['attempt_state'] != 'INCLUDED_SUCCESS'
                    or boundary['abci_code'] != '0'):
                raise ValueError()
            if any(boundary[key] is not False for key in (
                    'response_delivered', 'receipt_queried', 'asset_effect_visible',
                    'reusable_permit')):
                raise ValueError()
            if not HEX.fullmatch(boundary['tx_hash']) or not HEX.fullmatch(boundary['batch_id']):
                raise ValueError()
            if record['tx_hash'] != boundary['tx_hash'] or record['batch_id'] != boundary['batch_id']:
                raise ValueError()
            if type(boundary['height']) is not str or not DECIMAL.fullmatch(boundary['height']) or int(boundary['height']) == 0:
                raise ValueError()
            commit = boundary['commit']
            if type(commit) is not dict or set(commit) != COMMIT or not HEX.fullmatch(commit['record_hash']):
                raise ValueError()
            for key in ('command_seq', 'end_offset'):
                if type(commit[key]) is not str or not DECIMAL.fullmatch(commit[key]):
                    raise ValueError()
            records.append(record)
        if records[0]['phase'] != 'reserved':
            raise ValueError()
        final_phase = None
        if len(records) == 2:
            final = records[1]
            if (final['phase'] not in ('boundary_returned', 'boundary_error', 'panic')
                    or any(records[0][key] != final[key] for key in FIELDS - {'phase'})):
                raise ValueError()
            final_phase = final['phase']
        return dict(schema='s3-local-chain-commit-response-inspection/1',
                    boundary_sha256=expected_boundary_sha256,
                    file_sha256=hashlib.sha256(raw).hexdigest(),
                    final_phase=final_phase, partial_tail=bool(tail),
                    observation='RECORDED_RESPONSE_LOSS' if final_phase == 'boundary_error' else ('RECORDED_BOUNDARY_RETURN' if final_phase else 'UNKNOWN'),
                    response_delivered=False, receipt_queried=False,
                    asset_effect_verified=False, crash_verified=False,
                    authenticity_verified=False, fsync_verified=False,
                    replay_verified=False, reusable_permit=False,
                    durable_ack=False, DEV='NOT_RUN')
    except Exception:
        raise ValueError(ERROR) from None


def inspect(root, expected_boundary_sha256):
    try:
        return _inspect_file(root, expected_boundary_sha256,
                             'chain-commit-response.jsonl', inspect_bytes)
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
