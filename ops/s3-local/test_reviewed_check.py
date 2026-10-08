import copy
import unittest
from unittest.mock import patch
import approval_gate
import offline_check
import reviewed_check
import review_documents
from test_native_review import approved, uid
from test_review_documents import REVISIONS
import test_approval_gate as approval_fixtures
import test_review_subject as subject_fixtures
import test_offline_check as offline_fixtures
from test_preflight import PreflightFixture


class ReviewedCheckTest(PreflightFixture, unittest.TestCase):
    put = PreflightFixture.put
    seal = PreflightFixture.seal
    subject = subject_fixtures.ReviewSubjectTest.subject
    reader = approval_fixtures.ApprovalGateTest.reader

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
        offline_fixtures.OfflineCheckTest.prepare(self)
        self.issue = approved()
        self.calls = []
        from test_review_documents import documents
        self.rows = documents()
        bound = approval_gate.bound_subject(self.subject(), uid(5))
        for row, role in zip(self.rows, review_documents.AUTHORS):
            row['body'] = review_documents.approval_body(bound, role)

    def run_check(self):
        with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader):
            return reviewed_check.check(self.bundle, self.artifacts, self.pin,
                's3-dev-local/1', True, uid(5), REVISIONS, self.root, 'input.json',
                [], self.scratch, timeout=2)

    def test_authenticated_audits_surround_capture_child_and_cleanup(self):
        result = self.run_check()
        self.assertEqual(len(self.calls), 24)
        self.assertTrue(result['approval_audit']['approval_prerequisites_match'])
        self.assertTrue(result['offline_check']['byte_preflight']['input_set_byte_match'])
        for name in ['approval_verified', 'reusable_permit', 'services_started']:
            self.assertIs(result[name], False)
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_unapproved_candidate_never_starts_validator(self):
        self.issue['status'] = 'in_progress'
        with patch.object(offline_check, 'validate_captured') as child:
            with self.assertRaises(ValueError): self.run_check()
        child.assert_not_called()

    def test_revocation_or_binary_swap_during_child_rejects_and_cleans(self):
        original = offline_check.validate_captured
        binary = self.validator.read_bytes()
        rows = copy.deepcopy(self.rows)
        for mode in ['document', 'native', 'binary', 'io']:
            self.calls = []; self.issue = approved(); self.rows = copy.deepcopy(rows)
            self.validator.write_bytes(binary)
            def child(*args):
                result = original(*args)
                if mode == 'document': self.rows[0]['body'] = 'withdrawn'
                elif mode == 'native': self.issue['status'] = 'in_progress'
                elif mode == 'binary': self.validator.write_bytes(b'CHANGED')
                else: raise OSError('unavailable')
                return result
            with self.subTest(mode=mode), patch.object(offline_check, 'validate_captured', side_effect=child):
                with self.assertRaises((ValueError, OSError)): self.run_check()
            self.assertEqual(list(self.scratch.iterdir()), [])

    def test_child_failure_does_not_return_approval(self):
        with patch.object(offline_check, 'validate_captured', side_effect=ValueError('reject')):
            with self.assertRaises(ValueError): self.run_check()
        self.assertEqual(len(self.calls), 12)
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_different_final_audit_rejected(self):
        with patch.object(approval_gate, 'inspect', side_effect=[{'id':1}, {'id':2}]):
            with self.assertRaisesRegex(ValueError, 'APPROVAL_CHANGED_DURING_PREFLIGHT'):
                self.run_check()
        self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
