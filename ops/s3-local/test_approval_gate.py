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

    def test_final_authority_uses_fresh_documents_without_candidate_io(self):
        before=self.inspect();self.calls=[]
        with patch.object(gate.review_subject,'subject',side_effect=AssertionError('no candidate IO')):
            with patch.object(gate.Reader,'from_environment',return_value=self.reader):
                check=gate.final_authorizer(before,lambda:False)
                check()
                self.assertEqual(len(self.calls),9)
                self.rows[1]['body']=self.rows[1]['body'].replace('approved','revoked')
                with self.assertRaises(ValueError):check()
        self.assertFalse(before['reusable_permit'])

    def test_final_authority_changed_subject_revision_native_io_and_stop(self):
        for mode in ('subject','revision','native','io','stop'):
            self.setUp()
            before=self.inspect()
            if mode=='subject':
                value=gate.decode(self.rows[0]['body'].encode())
                value['subject']['manifest_sha256']='e'*64
                self.rows[0]['body']=gate.encode(value).decode()
            if mode=='revision':self.rows[1]['latestRevisionId']=uid(99)
            if mode=='native':self.issue['status']='in_progress'
            def read(path):
                if mode=='io':raise OSError()
                return self.reader(path)
            with patch.object(gate.Reader,'from_environment',return_value=read):
                with self.assertRaises((ValueError,OSError)):
                    gate.final_authorizer(before,lambda:mode=='stop')()

if __name__=='__main__': unittest.main()
