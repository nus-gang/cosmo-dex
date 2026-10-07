import base64
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from chain_commit_response_report import ERROR, inspect, inspect_bytes


class ChainCommitResponseReportTest(unittest.TestCase):
    def setUp(self):
        self.boundary = {
            'schema': 'sre-chain-commit-response-boundary/1', 'boundary': 'F08',
            'tx_hash': 'a' * 64, 'batch_id': 'b' * 64, 'height': '12',
            'abci_code': '0', 'attempt_state': 'INCLUDED_SUCCESS',
            'commit': {'command_seq': '3', 'record_hash': 'c' * 64, 'end_offset': '4'},
            'response_delivered': False, 'receipt_queried': False,
            'asset_effect_visible': False, 'reusable_permit': False,
        }
        raw = json.dumps(self.boundary, sort_keys=True, separators=(',', ':')).encode()
        self.sha = hashlib.sha256(raw).hexdigest()
        self.row = {
            'schema': 's3-local-chain-commit-response/1', 'phase': 'reserved',
            'boundary_base64': base64.b64encode(raw).decode(), 'boundary_sha256': self.sha,
            'tx_hash': 'a' * 64, 'batch_id': 'b' * 64,
            'response_delivered': False, 'receipt_queried': False,
            'asset_effect_verified': False, 'crash_verified': False,
            'durable_ack': False, 'DEV': 'NOT_RUN',
        }

    def encoded(self, rows, tail=b''):
        return b''.join(json.dumps(r, separators=(',', ':')).encode() + b'\n' for r in rows) + tail

    def test_error_and_partial_are_distinct(self):
        final = dict(self.row, phase='boundary_error')
        self.assertEqual(inspect_bytes(self.encoded([self.row, final]), self.sha)['observation'],
                         'RECORDED_RESPONSE_LOSS')
        self.assertEqual(inspect_bytes(self.encoded([self.row]), self.sha)['observation'], 'UNKNOWN')

    def test_tamper_and_claims_rejected(self):
        for changes in ({'response_delivered': True}, {'receipt_queried': True},
                        {'asset_effect_verified': True}, {'tx_hash': 'd' * 64},
                        {'phase': 'complete'}):
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect_bytes(self.encoded([dict(self.row, **changes)]), self.sha)
        changed = dict(self.boundary, asset_effect_visible=True)
        raw = json.dumps(changed, sort_keys=True, separators=(',', ':')).encode()
        row = dict(self.row, boundary_base64=base64.b64encode(raw).decode(),
                   boundary_sha256=hashlib.sha256(raw).hexdigest())
        with self.assertRaisesRegex(ValueError, ERROR):
            inspect_bytes(self.encoded([row]), row['boundary_sha256'])

    def test_duplicate_json_and_extra_record_rejected_while_partial_is_unknown(self):
        duplicate = self.encoded([self.row]).replace(b'"schema":', b'"schema":"x","schema":', 1)
        for raw in (duplicate, self.encoded([self.row, self.row, self.row])):
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect_bytes(raw, self.sha)
        self.assertEqual(inspect_bytes(self.encoded([self.row], b'{'), self.sha)['observation'],
                         'UNKNOWN')

    def test_file_permissions_and_hardlink_rejected(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td).resolve()
            root.chmod(0o700)
            report = root / 'chain-commit-response.jsonl'
            report.write_bytes(self.encoded([self.row]))
            report.chmod(0o600)
            self.assertEqual(inspect(root, self.sha)['observation'], 'UNKNOWN')
            report.chmod(0o644)
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect(root, self.sha)
            report.chmod(0o600)
            (root / 'alias').hardlink_to(report)
            with self.assertRaisesRegex(ValueError, ERROR):
                inspect(root, self.sha)


if __name__ == '__main__':
    unittest.main()
