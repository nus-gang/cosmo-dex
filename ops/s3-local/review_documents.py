"""Fresh independent document binding; reader authentication is caller-owned.

This is one prerequisite, not a runtime permission. Native decisions and an
explicit authenticated reader must be composed before a service can start.
"""
import hashlib
from manifest import decode, encode
from native_review import COMPANY, ISSUE, _uuid

AUTHORS = {'ceo': '67cfca39-4d60-4d4d-98d5-d57e186ae3e4',
           'cto': '890452a8-05a2-45f1-a2bf-b0c01278d521'}
KEYS = {role: 'runtime-' + role + '-approval' for role in AUTHORS}
MAX_SUBJECT = 16 * 1024 * 1024


def approval_body(subject, role):
    """Proposed document content; generating it never constitutes approval."""
    if role not in AUTHORS or not isinstance(subject, bytes) or not 0 < len(subject) <= MAX_SUBJECT:
        raise ValueError('INDEPENDENT_DOCUMENT_REQUIRED')
    parsed = decode(subject)
    if not isinstance(parsed, dict) or parsed.get('format') != 's3-local-review-subject/1':
        raise ValueError('INDEPENDENT_DOCUMENT_REQUIRED')
    return encode({'format': 's3-local-independent-approval/1',
        'company_id': COMPANY, 'issue_id': ISSUE, 'role': role,
        'decision': 'approved', 'subject_sha256': hashlib.sha256(subject).hexdigest(),
        'subject': parsed}).decode('utf-8')


def _projection(row, role, revision, expected_body):
    try:
        if (row['companyId'] != COMPANY or row['issueId'] != ISSUE or
                row['key'] != KEYS[role] or row['format'] != 'markdown' or
                row['latestRevisionId'] != revision or
                type(row['latestRevisionNumber']) is not int or row['latestRevisionNumber'] < 1 or
                row['createdByAgentId'] != AUTHORS[role] or row['createdByUserId'] is not None or
                row['updatedByAgentId'] != AUTHORS[role] or row['updatedByUserId'] is not None or
                row['body'] != expected_body):
            raise ValueError('INDEPENDENT_DOCUMENT_REQUIRED')
        return (_uuid(row['id']), revision, row['latestRevisionNumber'], expected_body)
    except (KeyError, TypeError, AttributeError):
        raise ValueError('INDEPENDENT_DOCUMENT_REQUIRED') from None


def inspect(read_document, subject, revisions):
    """Read both latest documents twice, with exact revision/author/body checks.

    read_document must fetch fresh authenticated records. Local JSON assertions
    cannot satisfy that trust boundary. The revisions are public handoff pins.
    A changed/revoked latest body is rejected even if an old revision approves.
    """
    if not isinstance(revisions, dict) or set(revisions) != set(AUTHORS):
        raise ValueError('INDEPENDENT_DOCUMENT_REQUIRED')
    bodies = {role: approval_body(subject, role) for role in AUTHORS}
    for revision in revisions.values():
        _uuid(revision)
    def capture():
        return {role: _projection(read_document('/api/issues/' + ISSUE + '/documents/' + KEYS[role]),
                role, revisions[role], bodies[role]) for role in AUTHORS}
    first = capture()
    second = capture()
    if first != second or first['ceo'][0] == first['cto'][0]:
        raise ValueError('INDEPENDENT_DOCUMENT_CHANGED')
    return {'independent_documents_match': True,
            'review_subject_sha256': hashlib.sha256(subject).hexdigest(),
            'revisions': dict(revisions), 'approval_verified': False,
            'services_started': False, 'DEV': 'NOT_RUN'}
