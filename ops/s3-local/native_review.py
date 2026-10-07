"""Native review prerequisite; trusted reader required, never a service permit.

The reader must fetch fresh authenticated Paperclip API records, not launcher
JSON or cached files. This module neither authenticates that reader nor checks
CEO/CTO subject attestations. Its result must not be used as a runtime pin.
"""
import copy
import re

COMPANY = 'a479c266-538c-4ed6-bbf0-1f96e1f67b3e'
ISSUE = 'f0147a00-13b4-4f47-ab83-36a246376544'
REVIEWERS = ('890452a8-05a2-45f1-a2bf-b0c01278d521',
             '3a95ba0a-5e42-43e6-a5f4-6582c879c7cb')
UUID = re.compile(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}')


def _uuid(value):
    if not isinstance(value, str) or not UUID.fullmatch(value):
        raise ValueError('NATIVE_REVIEW_REQUIRED')
    return value


def _projection(record):
    try:
        if (record['id'] != ISSUE or record['companyId'] != COMPANY or
                record['status'] != 'done'):
            raise ValueError('NATIVE_REVIEW_REQUIRED')
        policy = record['executionPolicy']
        stages = policy['stages']
        if (policy['mode'] != 'normal' or policy['commentRequired'] is not True or
                not isinstance(stages, list) or len(stages) != 2):
            raise ValueError('NATIVE_REVIEW_REQUIRED')
        stage_ids = []
        participant_ids = []
        for stage, reviewer in zip(stages, REVIEWERS):
            if (stage['type'] != 'review' or type(stage['approvalsNeeded']) is not int or
                    stage['approvalsNeeded'] != 1 or len(stage['participants']) != 1):
                raise ValueError('NATIVE_REVIEW_REQUIRED')
            participant = stage['participants'][0]
            if (participant['type'] != 'agent' or participant['agentId'] != reviewer or
                    participant['userId'] is not None):
                raise ValueError('NATIVE_REVIEW_REQUIRED')
            stage_ids.append(_uuid(stage['id']))
            participant_ids.append(_uuid(participant['id']))
        if len(set(stage_ids)) != 2 or len(set(participant_ids)) != 2:
            raise ValueError('NATIVE_REVIEW_REQUIRED')
        state = record['executionState']
        if (state['status'] != 'completed' or state['lastDecisionOutcome'] != 'approved' or
                state['completedStageIds'] != stage_ids or
                any(state[k] is not None for k in ('currentStageId', 'currentStageType',
                    'currentStageIndex', 'currentParticipant', 'reviewRequest'))):
            raise ValueError('NATIVE_REVIEW_REQUIRED')
        decision = _uuid(state['lastDecisionId'])
        return {'policy': copy.deepcopy(policy),
                'completed_stage_ids': stage_ids, 'last_decision_id': decision}
    except (KeyError, TypeError, IndexError, AttributeError):
        raise ValueError('NATIVE_REVIEW_REQUIRED') from None


def inspect(read_issue, expected_decision_id):
    """Check twice through a trusted fresh reader; no reusable authorization.

    expected_decision_id belongs to the independent exact-candidate handoff.
    Matching it does not bind that decision to any manifest by itself.
    """
    _uuid(expected_decision_id)
    path = '/api/issues/' + ISSUE
    before = _projection(read_issue(path))
    if before['last_decision_id'] != expected_decision_id:
        raise ValueError('NATIVE_REVIEW_DECISION_MISMATCH')
    after = _projection(read_issue(path))
    if before != after:
        raise ValueError('NATIVE_REVIEW_CHANGED')
    return {'native_review_complete': True,
            'last_decision_id': expected_decision_id,
            'completed_stage_ids': before['completed_stage_ids'],
            'approval_verified': False, 'services_started': False, 'DEV': 'NOT_RUN'}
