import copy
import unittest
from unittest.mock import Mock
import review_documents as gate
from manifest import encode
from test_native_review import uid

SUBJECT = encode({'format': 's3-local-review-subject/1', 'manifest_sha256':'a'*64,
                  'manifest_base64': 'e30=', 'components': {}})
REVISIONS = {'ceo': uid(10), 'cto': uid(11)}


def documents():
    return [{'id': uid(i), 'companyId': gate.COMPANY, 'issueId': gate.ISSUE,
        'key': gate.KEYS[role], 'format': 'markdown',
        'latestRevisionId': REVISIONS[role], 'latestRevisionNumber': 1,
        'createdByAgentId': author, 'createdByUserId': None,
        'updatedByAgentId': author, 'updatedByUserId': None,
        'body': gate.approval_body(SUBJECT, role)}
        for i, (role, author) in enumerate(gate.AUTHORS.items(), 1)]


class DocumentTest(unittest.TestCase):
    def check(self, rows):
        return gate.inspect(Mock(side_effect=rows), SUBJECT, REVISIONS)

    def test_independent_latest_documents_twice_are_not_a_permit(self):
        rows=documents(); reader=Mock(side_effect=rows*2)
        result=gate.inspect(reader, SUBJECT, REVISIONS)
        self.assertTrue(result['independent_documents_match'])
        self.assertFalse(result['approval_verified'])
        self.assertFalse(result['services_started'])
        self.assertEqual([c.args[0] for c in reader.call_args_list],
            ['/api/issues/'+gate.ISSUE+'/documents/'+gate.KEYS[r]
             for r in ['ceo','cto','ceo','cto']])

    def test_impersonation_foreign_scope_and_old_revision_rejected(self):
        for idx in range(2):
            for field, value in [('companyId',uid(99)), ('issueId',uid(99)),
                ('key','plan'), ('latestRevisionId',uid(99)), ('latestRevisionNumber',True),
                ('createdByAgentId',uid(99)), ('createdByUserId',uid(99)),
                ('updatedByAgentId',uid(99)), ('updatedByUserId',uid(99)), ('format','text')]:
                rows=documents(); rows[idx][field]=value
                with self.subTest(idx=idx,field=field), self.assertRaises(ValueError):
                    self.check(rows*2)

    def test_body_change_revocation_role_swap_and_subject_mismatch(self):
        for idx in range(2):
            for mutate in [lambda b:b+' ', lambda b:b.replace('approved','withdrawn'),
                           lambda b:b.replace('a'*64,'b'*64)]:
                rows=documents(); rows[idx]['body']=mutate(rows[idx]['body'])
                with self.assertRaises(ValueError): self.check(rows*2)
        rows=documents(); rows[0]['body']=rows[1]['body']
        with self.assertRaises(ValueError): self.check(rows*2)
        with self.assertRaises(ValueError):
            gate.inspect(Mock(side_effect=documents()*2), SUBJECT+b' ', REVISIONS)

    def test_recheck_detects_changes_and_io_failure(self):
        for idx in range(2):
            for field, value in [('body','withdrawn'),('latestRevisionId',uid(90)),('id',uid(90))]:
                before=documents(); after=copy.deepcopy(before); after[idx][field]=value
                with self.assertRaises(ValueError): self.check(before+after)
        with self.assertRaises(OSError): self.check(documents()+[OSError('offline')])
        rows=documents(); rows[1]['id']=rows[0]['id']
        with self.assertRaises(ValueError): self.check(rows*2)

    def test_missing_or_malformed_input_fails_before_reader(self):
        for revisions in [None, {}, {'ceo':uid(10)}, REVISIONS|{'other':uid(99)},
                          REVISIONS|{'cto':'invalid'}]:
            reader=Mock()
            with self.assertRaises(ValueError): gate.inspect(reader,SUBJECT,revisions)
            reader.assert_not_called()
        for row in [None, {}, [], {'body':'approved'}]:
            with self.assertRaises(ValueError): self.check([row]*4)
        for subject in [b'', b'{}', b'[]', 'text', b'a'*(gate.MAX_SUBJECT+1)]:
            reader=Mock()
            with self.assertRaises(ValueError): gate.inspect(reader,subject,REVISIONS)
            reader.assert_not_called()

if __name__ == '__main__': unittest.main()
