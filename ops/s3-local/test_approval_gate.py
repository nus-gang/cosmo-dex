import copy
import unittest
from unittest.mock import patch
import approval_gate as gate
import review_documents
from test_native_review import approved, uid
from test_review_documents import documents, REVISIONS
import test_review_subject as fixtures


class ApprovalGateTest(fixtures.ReviewSubjectTest):
    def setUp(self):
        super().setUp()
        self.rows = documents()
        self.bound = gate.bound_subject(self.subject(), uid(5))
        for row, role in zip(self.rows, review_documents.AUTHORS):
            row['body'] = review_documents.approval_body(self.bound, role)
        self.issue = approved()
        self.calls = []

    def reader(self, path):
        self.calls.append(path)
        if path.endswith(gate.native_review.ISSUE):
            return copy.deepcopy(self.issue)
        for row in self.rows:
            if path.endswith('/'+row['key']): return copy.deepcopy(row)
        raise AssertionError(path)

    def inspect(self, reader=None):
        return gate._inspect(reader or self.reader, self.bundle, self.artifacts,
            self.pin, 's3-dev-local/1', True, uid(5), REVISIONS)

    def test_composed_fresh_audit_is_not_runtime_permit(self):
        result = self.inspect()
        self.assertTrue(result['approval_prerequisites_match'])
        self.assertFalse(result['approval_verified'])
        self.assertFalse(result['reusable_permit'])
        self.assertFalse(result['services_started'])
        self.assertEqual(len(self.calls), 12)

    def test_old_unbound_and_different_decision_documents_rejected(self):
        for candidate in [self.subject(), gate.bound_subject(self.subject(),uid(99))]:
            self.rows[0]['body'] = review_documents.approval_body(candidate,'ceo')
            with self.assertRaises(ValueError): self.inspect()

    def test_binary_change_during_document_reads_rejected(self):
        def reader(path):
            result = self.reader(path)
            if len(self.calls) == 6: self.binary.write_bytes(b'changed')
            return result
        with self.assertRaises(ValueError): self.inspect(reader)

    def test_revocation_reopen_and_io_during_final_checks(self):
        for mode in ['document','native','io']:
            self.calls=[]; self.issue=approved(); self.rows=documents()
            for row, role in zip(self.rows,review_documents.AUTHORS):
                row['body']=review_documents.approval_body(self.bound,role)
            def reader(path):
                if len(self.calls)==6:
                    if mode=='document': self.rows[0]['body']='withdrawn'
                    elif mode=='native': self.issue['status']='in_progress'
                    else: raise OSError('offline')
                return self.reader(path)
            with self.subTest(mode=mode), self.assertRaises((ValueError,OSError)):
                self.inspect(reader)

    def test_public_api_uses_environment_reader(self):
        with patch.object(gate.Reader,'from_environment',return_value=self.reader) as factory:
            result=gate.inspect(self.bundle,self.artifacts,self.pin,'s3-dev-local/1',
                                True,uid(5),REVISIONS)
        factory.assert_called_once_with()
        self.assertTrue(result['approval_prerequisites_match'])

if __name__=='__main__': unittest.main()
