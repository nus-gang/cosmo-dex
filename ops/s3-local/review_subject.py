"""Offline review subject binding. No author authentication or service permit."""
import base64
import hashlib
import re
from manifest import COMPONENTS, decode, encode
from preflight import bounded, checked_root, verify, MAX_MANIFEST, MAX_DESCRIPTOR


def subject(bundle, artifacts, pin, profile, acknowledge):
    # Verify real artifacts first, then capture only bytes tied to that manifest.
    verify(bundle, artifacts, pin, profile, acknowledge)
    bundle = checked_root(bundle)
    raw = bounded(bundle, 'runtime-manifest.json', MAX_MANIFEST)
    if hashlib.sha256(raw).hexdigest() != pin:
        raise ValueError('MANIFEST_CHANGED')
    manifest = decode(raw)
    components = {}
    for name in COMPONENTS:
        path = manifest['components'][name]
        data = bounded(bundle, 'files/' + path, MAX_DESCRIPTOR)
        if hashlib.sha256(data).hexdigest() != manifest['files_sha256'][path]:
            raise ValueError('DESCRIPTOR_CHANGED')
        descriptor = decode(data)
        settings = descriptor['implementation_settings']
        argv = decode(settings['build_argv_json'])
        sources = decode(settings['approval_sources_json'])
        locks = decode(settings['implementation_locks_json'])
        if (not isinstance(argv, list) or not argv or
                not all(isinstance(x, str) and x for x in argv) or
                not isinstance(settings['toolchain'], str) or not settings['toolchain'] or
                not isinstance(sources, list) or not sources or
                not all(isinstance(x, str) and x for x in sources) or
                not isinstance(locks, dict) or not locks):
            raise ValueError('REVIEW_BUILD_METADATA_REQUIRED')
        components[name] = {
            'descriptor_base64': base64.b64encode(data).decode(),
            'descriptor_sha256': hashlib.sha256(data).hexdigest(),
            'head': descriptor['head'], 'tree': descriptor['tree'],
            'build_argv': argv, 'toolchain': settings['toolchain'],
            'implementation_locks': locks, 'approval_sources': sources,
            'artifacts_sha256': decode(settings['artifacts_sha256_json']),
        }
    return encode({'format': 's3-local-review-subject/1',
        'manifest_sha256': pin, 'manifest_base64': base64.b64encode(raw).decode(),
        'components': components})


def compare(current, independent_raw, independent_sha256):
    # The caller must obtain this digest from an authenticated independent
    # approval source. A file plus its self-asserted hash is not authorization.
    if (not isinstance(independent_sha256, str) or
            not re.fullmatch('[0-9a-f]{64}', independent_sha256) or
            not isinstance(independent_raw, bytes) or
            len(independent_raw) > 16 * 1024 * 1024 or
            hashlib.sha256(independent_raw).hexdigest() != independent_sha256 or
            current != independent_raw):
        raise ValueError('REVIEW_SUBJECT_MISMATCH')
    return {'review_subject_sha256': independent_sha256, 'subject_byte_match': True,
            'approval_verified': False, 'services_started': False, 'DEV': 'NOT_RUN'}
