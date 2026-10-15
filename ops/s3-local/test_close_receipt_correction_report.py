import base64
import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path

from close_receipt_correction_report import ERROR, inspect, inspect_bytes


class CloseReceiptCorrectionReportTest(unittest.TestCase):
    def setUp(self):
        self.boundary = {
            'schema': 'sre-close-receipt-correction-boundary/1', 'boundary': 'F13',
            'batch_id': 'a' * 64, 'receipt_sha256': 'b' * 64,
            'before_batch_count': '1', 'after_batch_count': '1',
            'before_correction_count': '0', 'after_correction_count': '0',
            'before_commit': {'command_seq': '3', 'record_hash': 'c' * 64, 'end_offset': '4'},
            'after_commit': {'command_seq': '4', 'record_hash': 'd' * 64, 'end_offset': '5'},
            'batch_state': 'CLOSING', 'batch_reason': 'ENGINE_APPLY_PENDING',
            'correction_plan_visible': False, 'replacement_seq_created': False,
            'apply_called': False, 'reusable_permit': False,
        }
        raw = json.dumps(self.boundary, sort_keys=True, separators=(',', ':')).encode()
        self.sha = hashlib.sha256(raw).hexdigest()
        self.row = {
            'schema': 's3-local-close-receipt-correction/1', 'phase': 'reserved',
            'boundary_base64': base64.b64encode(raw).decode(), 'boundary_sha256': self.sha,
            'receipt_sha256': 'b' * 64, 'batch_id': 'a' * 64, 'before_batch_count': '1',
            'correction_plan_visible': False, 'replacement_seq_created': False,
            'apply_called': False, 'crash_verified': False, 'durable_ack': False,
            'DEV': 'NOT_RUN',
        }

    def encoded(self, rows, tail=b''):
        return b''.join(json.dumps(row, separators=(',', ':')).encode() + b'\n' for row in rows) + tail

    def test_return_and_partial_are_distinct(self):
        final = dict(self.row, phase='boundary_returned')
        self.assertEqual(inspect_bytes(self.encoded([self.row, final]), self.sha)['observation'],
                         'RECORDED_BOUNDARY_RETURN')
        self.assertEqual(inspect_bytes(self.encoded([self.row]), self.sha)['observation'], 'UNKNOWN')

    def test_correction_replacement_and_apply_claims_are_rejected(self):
        for change in ({'correction_plan_visible': True}, {'replacement_seq_created': True},
                       {'apply_called': True}, {'crash_verified': True},
                       {'batch_id': 'e' * 64}):
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect_bytes(self.encoded([dict(self.row, **change)]), self.sha)
        changed = dict(self.boundary, after_batch_count='2')
        raw = json.dumps(changed, sort_keys=True, separators=(',', ':')).encode()
        row = dict(self.row, boundary_base64=base64.b64encode(raw).decode(),
                   boundary_sha256=hashlib.sha256(raw).hexdigest())
        with self.assertRaisesRegex(ValueError, ERROR):
            inspect_bytes(self.encoded([row]), row['boundary_sha256'])

    def test_commit_order_duplicate_json_and_extra_record_are_rejected(self):
        changed = dict(self.boundary, after_commit=self.boundary['before_commit'])
        raw = json.dumps(changed, sort_keys=True, separators=(',', ':')).encode()
        row = dict(self.row, boundary_base64=base64.b64encode(raw).decode(),
                   boundary_sha256=hashlib.sha256(raw).hexdigest())
        duplicate = self.encoded([self.row]).replace(b'"schema":', b'"schema":"x","schema":', 1)
        for candidate, digest in ((self.encoded([row]), row['boundary_sha256']),
                                  (duplicate, self.sha),
                                  (self.encoded([self.row] * 3), self.sha)):
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect_bytes(candidate, digest)

    def test_file_permissions_and_hardlink_are_rejected(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td).resolve()
            root.chmod(0o700)
            report = root / 'close-receipt-correction.jsonl'
            report.write_bytes(self.encoded([self.row]))
            report.chmod(0o600)
            self.assertEqual(inspect(root, self.sha)['observation'], 'UNKNOWN')
            report.chmod(0o644)
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect(root, self.sha)
            report.chmod(0o600)
            os.link(report, root / 'alias')
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect(root, self.sha)


if __name__ == '__main__':
    unittest.main()
