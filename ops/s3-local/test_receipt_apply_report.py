import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import receipt_apply_report as m


class ReceiptApplyReportTests(unittest.TestCase):
    def setUp(self):
        self.boundary = dict(
            schema='sre-receipt-apply-boundary/1', boundary='F09',
            batch_id='a' * 64, disposition='COMMITTED', receipt_sha256='b' * 64,
            before_commit=dict(command_seq='7', record_hash='c' * 64, end_offset='80'),
            after_commit=dict(command_seq='8', record_hash='d' * 64, end_offset='96'),
            apply_called=False, reusable_permit=False)
        self.first, self.sha = self.row(self.boundary)

    def row(self, boundary):
        data = json.dumps(boundary, sort_keys=True, separators=(',', ':')).encode()
        sha = hashlib.sha256(data).hexdigest()
        return dict(schema='s3-local-receipt-apply/1', phase='reserved',
                    boundary_base64=base64.b64encode(data).decode(),
                    boundary_sha256=sha, receipt_sha256=boundary['receipt_sha256'],
                    batch_id=boundary['batch_id'], disposition=boundary['disposition'],
                    apply_called=False, crash_verified=False, durable_ack=False,
                    DEV='NOT_RUN'), sha

    @staticmethod
    def raw(*rows):
        return b''.join(json.dumps(row).encode() + b'\n' for row in rows)

    def test_partial_and_final_states_never_claim_apply(self):
        for tail in (b'', b'{', b'{"phase":"panic"}'):
            result = m.inspect_bytes(self.raw(self.first) + tail, self.sha)
            self.assertEqual(result['observation'], 'UNKNOWN')
        for phase in ('boundary_returned', 'boundary_error', 'panic'):
            result = m.inspect_bytes(self.raw(self.first, dict(self.first, phase=phase)), self.sha)
            self.assertEqual(result['final_phase'], phase)
            for key in ('apply_verified', 'crash_verified', 'receipt_durability_verified',
                        'authenticity_verified', 'fsync_verified', 'replay_verified',
                        'reusable_permit', 'durable_ack'):
                self.assertIs(result[key], False)

    def test_boundary_identity_and_commit_progress(self):
        changes = [
            {'boundary': 'F08'}, {'batch_id': 'A' * 64}, {'disposition': 'UNKNOWN'},
            {'receipt_sha256': 'x'}, {'apply_called': True}, {'reusable_permit': True},
            {'before_commit': dict(self.boundary['before_commit'], command_seq='07')},
            {'after_commit': dict(self.boundary['after_commit'], command_seq='7')},
            {'after_commit': dict(self.boundary['after_commit'], end_offset='80')},
            {'after_commit': dict(self.boundary['after_commit'], record_hash='x')},
            {'extra': 'secret'},
        ]
        for change in changes:
            row, sha = self.row(dict(self.boundary, **change))
            with self.assertRaisesRegex(ValueError, m.ERROR):
                m.inspect_bytes(self.raw(row), sha)
        void, sha = self.row(dict(self.boundary, disposition='VOID'))
        m.inspect_bytes(self.raw(void), sha)

    def test_record_mismatch_duplicate_and_partial_rejection(self):
        for change in ({'phase': 'panic'}, {'schema': 'other'}, {'batch_id': 'e' * 64},
                       {'boundary_sha256': '0' * 64}, {'boundary_base64': '!'},
                       {'apply_called': True}, {'durable_ack': 0}, {'extra': 0}):
            with self.assertRaises(ValueError):
                m.inspect_bytes(self.raw(dict(self.first, **change)), self.sha)
        duplicate = self.raw(self.first).replace(b'"phase": "reserved"',
                                                  b'"phase":"reserved","phase":"reserved"')
        for raw in (b'', b'x' * (m.CAP + 1), duplicate, self.raw(self.first) * 2,
                    self.raw(self.first) * 3,
                    self.raw(self.first, dict(self.first, phase='panic')) + b'x'):
            with self.assertRaises(ValueError):
                m.inspect_bytes(raw, self.sha)

    def test_readonly_and_unsafe_file_rejection(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); root.chmod(0o700)
            report = root / 'receipt-apply.jsonl'
            raw = self.raw(self.first); report.write_bytes(raw); report.chmod(0o600)
            before = report.stat()
            self.assertEqual(m.inspect(root, self.sha)['observation'], 'UNKNOWN')
            after = report.stat()
            self.assertEqual((before.st_ino, before.st_mtime_ns, before.st_ctime_ns),
                             (after.st_ino, after.st_mtime_ns, after.st_ctime_ns))
            self.assertEqual(report.read_bytes(), raw)
            report.chmod(0o644)
            with self.assertRaises(ValueError): m.inspect(root, self.sha)
            report.chmod(0o600); os.link(report, root / 'alias')
            with self.assertRaises(ValueError): m.inspect(root, self.sha)
            (root / 'alias').unlink(); report.unlink(); os.mkfifo(report, 0o600)
            with self.assertRaises(ValueError): m.inspect(root, self.sha)
            report.unlink(); report.symlink_to(root / 'missing')
            with self.assertRaises(ValueError): m.inspect(root, self.sha)

    def test_path_replacement_and_open_stdin_cli_reject(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); root.chmod(0o700)
            report = root / 'receipt-apply.jsonl'
            report.write_bytes(self.raw(self.first)); report.chmod(0o600)
            read = os.read
            def replace(fd, size):
                data = read(fd, size)
                if data:
                    report.rename(root / 'saved')
                    report.write_bytes(data); report.chmod(0o600)
                return data
            with patch('storage_fault_report.os.read', replace):
                with self.assertRaises(ValueError): m.inspect(root, self.sha)
        child = subprocess.Popen([sys.executable, m.__file__, 'inspect'],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE)
        try:
            child.wait(timeout=2)
            self.assertEqual(child.returncode, 2)
            self.assertEqual(child.stdout.read(), b'')
            self.assertEqual(child.stderr.read(), (m.ERROR + '\n').encode())
        finally:
            if child.poll() is None:
                child.kill(); child.wait()
            child.stdin.close(); child.stdout.close(); child.stderr.close()


if __name__ == '__main__':
    unittest.main()
