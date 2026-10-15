import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import rpc_response_report as m


class RpcResponseReportTests(unittest.TestCase):
    def setUp(self):
        self.boundary = dict(schema='sre-rpc-response-boundary/1', boundary='F06', state='SUBMISSION_UNKNOWN',
            tx_hash='a'*64, broadcast_count='1', raw_len='123', stored_intent_sha256='b'*64,
            request_sha256='a'*64, headers_sha256=None, partial_json_sha256=None, partial_json_len='0',
            transport_callback_called=True, socket_verified=False, response_complete=False,
            attempt_resolved=False, reusable_permit=False)
        self.first, self.sha = self.row(self.boundary)

    def row(self, boundary):
        data = json.dumps(boundary, sort_keys=True, separators=(',', ':')).encode()
        sha = hashlib.sha256(data).hexdigest()
        return dict(schema='s3-local-rpc-response/1', phase='reserved', boundary_base64=base64.b64encode(data).decode(),
            boundary_sha256=sha, socket_verified=False, response_complete=False, attempt_resolved=False,
            asset_effect_verified=False, crash_verified=False, durable_ack=False, DEV='NOT_RUN'), sha

    def raw(self, *rows):
        return b''.join(json.dumps(row).encode() + b'\n' for row in rows)

    def test_f06_f07_and_partial_states(self):
        self.assertEqual(m.inspect_bytes(self.raw(self.first), self.sha)['observation'], 'UNKNOWN')
        for phase in ('boundary_returned', 'boundary_error', 'panic'):
            result = m.inspect_bytes(self.raw(self.first, dict(self.first, phase=phase)), self.sha)
            self.assertEqual(result['observation'], 'RECORDED_RESPONSE_BOUNDARY')
            for key in ('socket_verified','response_complete','attempt_resolved','asset_effect_verified','crash_verified','authenticity_verified','fsync_verified','replay_verified','reusable_permit','durable_ack'):
                self.assertIs(result[key], False)
        f07 = dict(self.boundary, boundary='F07', headers_sha256='c'*64,
                   partial_json_sha256='d'*64, partial_json_len='17')
        row, sha = self.row(f07)
        m.inspect_bytes(self.raw(row, dict(row, phase='boundary_error')), sha)

    def test_identity_phase_and_caps_rejected(self):
        changes = ({'state':'PREPARED'}, {'boundary':'F08'}, {'broadcast_count':'0'},
                   {'request_sha256':'c'*64}, {'raw_len':'139265'}, {'partial_json_len':'1'},
                   {'socket_verified':True}, {'response_complete':True}, {'attempt_resolved':True},
                   {'transport_callback_called':False}, {'extra':'secret'})
        for change in changes:
            row, sha = self.row(dict(self.boundary, **change))
            with self.assertRaisesRegex(ValueError, m.ERROR):
                m.inspect_bytes(self.raw(row), sha)
        f07 = dict(self.boundary, boundary='F07', headers_sha256='c'*64,
                   partial_json_sha256='d'*64, partial_json_len='65537')
        row, sha = self.row(f07)
        with self.assertRaises(ValueError):
            m.inspect_bytes(self.raw(row), sha)

    def test_forged_mixed_and_duplicate_records(self):
        for change in ({'phase':'panic'}, {'boundary_sha256':'0'*64}, {'boundary_base64':'!'},
                       {'socket_verified':0}, {'durable_ack':True}, {'extra':0}):
            with self.assertRaises(ValueError):
                m.inspect_bytes(self.raw(dict(self.first, **change)), self.sha)
        duplicate = self.raw(self.first).replace(b'"phase": "reserved"', b'"phase":"reserved","phase":"reserved"')
        for raw in (b'', b'x'*(m.CAP+1), duplicate, self.raw(self.first)*2, self.raw(self.first)*3):
            with self.assertRaises(ValueError):
                m.inspect_bytes(raw, self.sha)

    def test_readonly_file_and_cli_rejection(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); root.chmod(0o700)
            path = root/'rpc-response.jsonl'; raw = self.raw(self.first)
            path.write_bytes(raw); path.chmod(0o600); before = path.stat()
            self.assertEqual(m.inspect(root, self.sha)['observation'], 'UNKNOWN')
            after = path.stat()
            self.assertEqual((before.st_ino,before.st_mtime_ns,before.st_ctime_ns),
                             (after.st_ino,after.st_mtime_ns,after.st_ctime_ns))
            self.assertEqual(path.read_bytes(), raw)
            path.chmod(0o644)
            with self.assertRaises(ValueError): m.inspect(root, self.sha)
            path.chmod(0o600); os.link(path, root/'alias')
            with self.assertRaises(ValueError): m.inspect(root, self.sha)
        child = subprocess.Popen([sys.executable, m.__file__, 'inspect'], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            child.wait(timeout=2); self.assertEqual(child.returncode, 2)
            self.assertEqual(child.stdout.read(), b'')
            self.assertEqual(child.stderr.read(), (m.ERROR+'\n').encode())
        finally:
            if child.poll() is None: child.kill(); child.wait()
            child.stdin.close(); child.stdout.close(); child.stderr.close()


if __name__ == '__main__':
    unittest.main()
