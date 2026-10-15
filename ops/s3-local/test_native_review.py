import copy
import unittest
from unittest.mock import Mock
import native_review as gate


def uid(n):
    return '00000000-0000-0000-0000-' + str(n).zfill(12)


def approved():
    return {'id': gate.ISSUE, 'companyId': gate.COMPANY, 'status': 'done',
        'executionPolicy': {'mode': 'normal', 'commentRequired': True, 'stages': [
            {'id': uid(i), 'type': 'review', 'approvalsNeeded': 1,
             'participants': [{'id': uid(i+2), 'type': 'agent', 'userId': None,
                               'agentId': reviewer}]}
            for i, reviewer in enumerate(gate.REVIEWERS, 1)]},
        'executionState': {'status': 'completed', 'lastDecisionOutcome': 'approved',
            'lastDecisionId': uid(5), 'completedStageIds': [uid(1), uid(2)],
            'currentStageId': None, 'currentStageType': None, 'currentStageIndex': None,
            'currentParticipant': None, 'reviewRequest': None}}


class NativeReviewTest(unittest.TestCase):
    def check(self, row):
        reader = Mock(return_value=row)
        return gate.inspect(reader, uid(5)), reader

    def test_complete_requires_two_fresh_reads_and_is_not_a_permit(self):
        report, reader = self.check(approved())
        self.assertEqual(reader.call_args_list, [unittest.mock.call('/api/issues/'+gate.ISSUE)]*2)
        self.assertTrue(report['native_review_complete'])
        self.assertFalse(report['approval_verified'])
        self.assertFalse(report['services_started'])

    def test_reopened_rejected_incomplete_wrong_company_or_issue(self):
        for field, value in [('status','in_progress'), ('companyId',uid(9)), ('id',uid(9))]:
            row = approved(); row[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError): self.check(row)
        for field, value in [('status','idle'), ('lastDecisionOutcome','changes_requested'),
                ('lastDecisionId',uid(6)), ('completedStageIds',[uid(1)]),
                ('completedStageIds',[uid(2),uid(1)]), ('currentStageIndex',0),
                ('reviewRequest',{})]:
            row=approved(); row['executionState'][field]=value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError): self.check(row)

    def test_policy_order_identity_threshold_and_duplicates(self):
        changes = [lambda p:p['stages'].reverse(),
            lambda p:p.update(commentRequired=False),
            lambda p:p['stages'][0].update(approvalsNeeded=True),
            lambda p:p['stages'][0].update(approvalsNeeded=0),
            lambda p:p['stages'][0].update(type='monitor'),
            lambda p:p['stages'][1].update(id=uid(1)),
            lambda p:p['stages'][1]['participants'][0].update(id=uid(3)),
            lambda p:p['stages'][0]['participants'][0].update(agentId=uid(9)),
            lambda p:p['stages'][0]['participants'][0].update(userId=uid(9)),
            lambda p:p['stages'][0]['participants'].append({})]
        for change in changes:
            row=approved(); change(row['executionPolicy'])
            with self.assertRaises(ValueError): self.check(row)

    def test_change_between_reads_and_io_fail_closed(self):
        for mutate in [lambda r:r.update(status='in_progress'),
                lambda r:r['executionState'].update(lastDecisionId=uid(8)),
                lambda r:r['executionPolicy']['stages'][0]['participants'][0].update(id=uid(8))]:
            first=approved(); second=copy.deepcopy(first); mutate(second)
            with self.assertRaises(ValueError):
                gate.inspect(Mock(side_effect=[first, second]), uid(5))
        with self.assertRaises(OSError):
            gate.inspect(Mock(side_effect=[approved(), OSError('offline')]), uid(5))

    def test_malformed_records_and_untrusted_decision_shape(self):
        for row in [None, {}, [], {'executionState':{}}, approved() | {'executionPolicy':None}]:
            with self.assertRaises(ValueError): self.check(row)
        reader=Mock()
        for value in [None, '', 'self-approved', True]:
            with self.assertRaises(ValueError): gate.inspect(reader, value)
        reader.assert_not_called()

if __name__ == '__main__': unittest.main()
