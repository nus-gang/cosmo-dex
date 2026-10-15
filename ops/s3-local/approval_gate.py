"""Fresh composed approval audit. No process launch or reusable runtime permit."""
import hashlib
from contextlib import contextmanager
from contextvars import ContextVar
import native_review
import review_documents
import review_subject
from manifest import decode, encode
from paperclip_reader import Reader


def bound_subject(candidate, decision_id):
    """Body for independent authors to approve after native review completes."""
    native_review._uuid(decision_id)
    value = decode(candidate)
    if (not isinstance(value, dict) or
            value.get('format') != 's3-local-review-subject/1' or
            'native_review' in value):
        raise ValueError('APPROVAL_SUBJECT_INVALID')
    value['native_review'] = {'issue_id': native_review.ISSUE,
        'last_decision_id': decision_id, 'reviewers': list(native_review.REVIEWERS)}
    return encode(value)


def _inspect(reader, bundle, artifacts, pin, profile, acknowledge, decision_id, revisions):
    # Internal reader seam for pure tests; public entry always constructs the
    # authenticated reader from the current run, never from manifest fields.
    candidate = review_subject.subject(bundle, artifacts, pin, profile, acknowledge)
    subject = bound_subject(candidate, decision_id)
    native_before = native_review.inspect(reader, decision_id)
    documents_before = review_documents.inspect(reader, subject, revisions)
    if review_subject.subject(bundle, artifacts, pin, profile, acknowledge) != candidate:
        raise ValueError('APPROVAL_CANDIDATE_CHANGED')
    documents_after = review_documents.inspect(reader, subject, revisions)
    native_after = native_review.inspect(reader, decision_id)
    if documents_before != documents_after or native_before != native_after:
        raise ValueError('APPROVAL_CHANGED')
    return {'format': 's3-local-approval-audit/1',
        'approval_prerequisites_match': True,
        'review_subject_sha256': hashlib.sha256(subject).hexdigest(),
        'manifest_sha256': pin, 'native_decision_id': decision_id,
        'independent_revisions': dict(revisions),
        'approval_verified': False, 'reusable_permit': False,
        'services_started': False, 'DEV': 'NOT_RUN'}


_PRIVATE_READER = ContextVar('s3_private_approval_reader', default=None)


@contextmanager
def private_transport(endpoint):
    """Scope the managed runtime's reader; never fall back after IPC failure.

    Endpoint is supplied by the current-run broker to the managed command.
    Same-uid trust and broker lifetime restrictions remain applicable.
    """
    from private_reader import PrivateReader
    reader = PrivateReader(endpoint)
    token = _PRIVATE_READER.set(reader)
    try:
        yield
    finally:
        _PRIVATE_READER.reset(token)


def inspect(bundle, artifacts, pin, profile, acknowledge, decision_id, revisions):
    reader = _PRIVATE_READER.get()
    if reader is None:
        reader = Reader.from_environment()
    return _inspect(reader, bundle, artifacts, pin, profile,
                    acknowledge, decision_id, revisions)


def _recheck(reader, audit):
    """Fresh authorization only; the prior audit supplies an identity, not authority.

    Re-read native state and both canonical documents. Reconstruct the exact
    subject from a fresh document and compare its digest to the byte audit.
    No candidate hashing, saved approval body, local permit, or reader cache.
    The caller checks the resource deadline AFTER these authenticated reads.
    """
    if (audit.get('format') != 's3-local-approval-audit/1' or
            audit.get('approval_prerequisites_match') is not True or
            audit.get('reusable_permit') is not False):
        raise ValueError('APPROVAL_CHANGED')
    decision = audit['native_decision_id']
    before = native_review.inspect(reader, decision)
    row = reader('/api/issues/' + native_review.ISSUE + '/documents/' + review_documents.KEYS['ceo'])
    subject = encode(decode(row['body'].encode())['subject'])
    if hashlib.sha256(subject).hexdigest() != audit['review_subject_sha256']:
        raise ValueError('APPROVAL_CHANGED')
    review_documents.inspect(reader, subject, dict(audit['independent_revisions']))
    if native_review.inspect(reader, decision) != before:
        raise ValueError('APPROVAL_CHANGED')


def recheck(audit):
    """Use fresh current-run authentication, including scoped private transport."""
    reader = _PRIVATE_READER.get()
    if reader is None:
        reader = Reader.from_environment()
    _recheck(reader, audit)


def final_authorizer(audit, stop):
    # Private callback wiring; every invocation does authenticated IO. No result
    # of this closure is a permit. Copy the subject identity across callbacks.
    import copy
    expected = copy.deepcopy(audit)
    def authorize():
        if stop():
            raise ValueError('APPROVAL_CHANGED')
        recheck(expected)
        if stop():
            raise ValueError('APPROVAL_CHANGED')
    return authorize
