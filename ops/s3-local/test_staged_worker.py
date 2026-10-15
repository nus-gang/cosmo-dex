import base64
import hashlib
import unittest
from unittest.mock import patch
import approval_gate
import review_documents
import staged_worker
from manifest import aggregate, decode, encode
from test_native_review import uid
from test_review_documents import REVISIONS
import test_reviewed_check as fixtures

class StagedWorkerTest(fixtures.PreflightFixture, unittest.TestCase):
    subject = fixtures.ReviewedCheckTest.subject
    reader = fixtures.ReviewedCheckTest.reader
    def setUp(self):
        super().setUp()
        import json
        from manifest import COMPONENTS, encode, aggregate
        for name in COMPONENTS:
            path = self.manifest['components'][name]
            descriptor = json.loads((self.bundle/'files'/path).read_bytes())
            descriptor['implementation_settings'].update({
                'build_argv_json': encode(['TEST_ONLY', '--locked']).decode(),
                'toolchain': 'TEST_ONLY',
                'approval_sources_json': encode(['TEST_ONLY_NOT_APPROVAL']).decode(),
                'implementation_locks_json': encode({'TEST_ONLY': 'c'*64}).decode()})
            self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        fixtures.offline_fixtures.OfflineCheckTest.prepare(self)
        self.issue = fixtures.approved()
        self.calls = []
        from test_review_documents import documents
        self.rows = documents()
        bound = approval_gate.bound_subject(self.subject(), uid(5))
        for row, role in zip(self.rows, review_documents.AUTHORS):
            row['body'] = review_documents.approval_body(bound, role)

        self.worker = self.artifacts / staged_worker.WORKER
        self.worker.write_bytes(b'TEST_ONLY_NEVER_EXECUTED')
        path = staged_worker.DESCRIPTOR
        descriptor = decode((self.bundle/'files'/path).read_bytes())
        inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
        inventory[staged_worker.WORKER] = hashlib.sha256(self.worker.read_bytes()).hexdigest()
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode(inventory).decode()
        self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        capture = decode(self.input_file.read_bytes())
        capture['runtime_manifest'] = base64.b64encode((self.bundle/'runtime-manifest.json').read_bytes()).decode()
        capture['files'][path] = base64.b64encode((self.bundle/'files'/path).read_bytes()).decode()
        self.input_file.write_bytes(encode(capture))
        bound = approval_gate.bound_subject(self.subject(), uid(5))
        for row, role in zip(self.rows, review_documents.AUTHORS):
            row['body'] = review_documents.approval_body(bound, role)
    def stage(self, **kw):
        args = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin,
            profile='s3-dev-local/1', acknowledge=True, decision_id=uid(5),
            revisions=REVISIONS, inputs=self.root, input_name='input.json', scratch=self.scratch)
        args.update(kw)
        return staged_worker.stage(**args)
    def test_exact_capture_private_copy_survives_source_swap_and_is_removed(self):
        raw = self.input_file.read_bytes(); binary = self.worker.read_bytes()
        with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader):
            with self.stage() as staged:
                self.assertEqual(len(self.calls), 24)
                self.worker.write_bytes(b'REPLACEMENT')
                self.input_file.write_bytes(b'REPLACEMENT')
                self.assertEqual(staged.capture, raw)
                self.assertEqual(staged.capture_sha256, hashlib.sha256(raw).hexdigest())
                self.assertEqual(staged.executable.read_bytes(), binary)
                self.assertEqual(staged.executable_sha256, hashlib.sha256(binary).hexdigest())
                self.assertEqual(staged.executable.stat().st_mode & 0o777, 0o500)
                self.assertEqual(staged.executable.parent.stat().st_mode & 0o777, 0o700)
            self.assertFalse(staged.executable.exists())
        self.assertEqual(list(self.scratch.iterdir()), [])
        self.assertTrue(self.worker.exists())
    def test_first_denial_or_missing_optin_never_stages(self):
        with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader):
            for kw in [dict(acknowledge=False), dict(profile='standard')]:
                with self.assertRaises(ValueError), self.stage(**kw): self.fail('yielded')
            self.issue['status'] = 'in_progress'
            with self.assertRaises(ValueError), self.stage(): self.fail('yielded')
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_post_capture_change_rejected_without_yield(self):
        original = staged_worker.verify_input_set
        def capture(*args):
            result = original(*args); self.worker.write_bytes(b'CHANGED'); return result
        with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader), \
             patch.object(staged_worker, 'verify_input_set', side_effect=capture):
            with self.assertRaisesRegex(ValueError, 'WORKER_BYTES_CHANGED'), self.stage(): self.fail('yielded')
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_final_denial_changed_audit_and_io_remove_private_copy(self):
        for final in [ValueError('revoked'), OSError('API unavailable'), {'changed':True}]:
            with patch.object(approval_gate, 'inspect', side_effect=[{'before':True}, final]):
                with self.assertRaises((ValueError, OSError)), self.stage(): self.fail('yielded')
            self.assertEqual(list(self.scratch.iterdir()), [])
    def test_consumer_failure_or_interrupt_cleans_only_staged_files(self):
        for error in [RuntimeError('failure'), KeyboardInterrupt()]:
            with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader):
                with self.assertRaises(type(error)), self.stage() as staged:
                    raise error
            self.assertFalse(staged.executable.exists())
            self.assertTrue(self.worker.exists()); self.assertTrue(self.input_file.exists())
            self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
