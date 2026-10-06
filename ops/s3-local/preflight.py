#!/usr/bin/env python3
"""Pure runtime byte preflight. No service, key, home, port binding or approval."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import stat
from manifest import COMPONENTS, CANDIDATE, BASELINE, PREFIX, aggregate, decode, relative

MAX_MANIFEST = 262144
MAX_DESCRIPTOR = 1048576


def checked_root(root):
    root = Path(root)
    if not root.is_absolute() or str(root.resolve(strict=True)) != str(root):
        raise ValueError('CANONICAL_ROOT_REQUIRED')
    if not root.is_dir():
        raise ValueError('DIRECTORY_REQUIRED')
    return root


def bounded(root, path, limit):
    # Component walk rejects symlinks before opening, then fd metadata binds read.
    relative(path)
    import os
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        parts = path.split('/')
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        file_fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        try:
            before = os.fstat(file_fd)
            if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > limit:
                raise ValueError('FILE_POLICY')
            with os.fdopen(file_fd, 'rb', closefd=False) as f:
                raw = f.read(limit + 1)
            after = os.fstat(file_fd)
            if len(raw) > limit or (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                raise ValueError('FILE_CHANGED_OR_OVERSIZE')
            return raw
        finally:
            os.close(file_fd)
    finally:
        os.close(fd)


def file_digest(root, path):
    # Bounded memory even for compiled Go binaries. Reuse descriptor-relative
    # traversal; no shell, execution, permissions change or file creation.
    import os
    relative(path)
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        parts = path.split('/')
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        item = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        try:
            before = os.fstat(item)
            if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not 0 < before.st_size <= 536870912:
                raise ValueError('ARTIFACT_POLICY')
            h = hashlib.sha256()
            count = 0
            while True:
                b = os.read(item, 1048576)
                if not b:
                    break
                count += len(b)
                if count > before.st_size:
                    raise ValueError('ARTIFACT_CHANGED')
                h.update(b)
            after = os.fstat(item)
            if count != before.st_size or (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                raise ValueError('ARTIFACT_CHANGED')
            return h.hexdigest()
        finally:
            os.close(item)
    finally:
        os.close(fd)


def verify(bundle, artifacts, pin, profile, acknowledge):
    if profile != 's3-dev-local/1' or acknowledge is not True:
        raise ValueError('TWO_OPT_INS_REQUIRED')
    if not isinstance(pin, str) or not re.fullmatch('[0-9a-f]{64}', pin):
        raise ValueError('INDEPENDENT_PIN_REQUIRED')
    bundle, artifacts = checked_root(bundle), checked_root(artifacts)
    raw = bounded(bundle, 'runtime-manifest.json', MAX_MANIFEST)
    if hashlib.sha256(raw).hexdigest() != pin:
        raise ValueError('RUNTIME_PIN_MISMATCH')
    m = decode(raw)
    if set(m) != {'format', 'scope', 'candidate_manifest_sha256', 'contract_sha256', 'files_sha256', 'components'} or m['format'] != 's3-dev-local-runtime/1' or m['scope'] != 'REVIEWED_RUNTIME' or m['candidate_manifest_sha256'] != CANDIDATE:
        raise ValueError('MANIFEST_POLICY')
    expected_components = {c: 'chain/local-demo/components/' + c + '.json' for c in COMPONENTS}
    if m['components'] != expected_components or len(m['files_sha256']) != 218 or aggregate(m['files_sha256']) != m['contract_sha256']:
        raise ValueError('MANIFEST_FILE_SET')
    # Independently anchor the exact 213 inherited paths to approved manifests.
    baseline_raw = bounded(bundle, 'files/protocol/s3/manifest.json', MAX_MANIFEST)
    candidate_raw = bounded(bundle, 'files/' + PREFIX + 'MANIFEST.json', MAX_MANIFEST)
    if hashlib.sha256(baseline_raw).hexdigest() != BASELINE or hashlib.sha256(candidate_raw).hexdigest() != CANDIDATE:
        raise ValueError('APPROVED_MANIFEST_MISMATCH')
    expected = dict(decode(baseline_raw)['files_sha256'])
    expected.update({PREFIX + p: h for p, h in decode(candidate_raw)['files_sha256'].items()})
    expected.update({'protocol/s3/manifest.json': BASELINE, PREFIX + 'MANIFEST.json': CANDIDATE})
    if set(m['files_sha256']) != set(expected) | set(expected_components.values()) or any(m['files_sha256'][p] != h for p, h in expected.items()):
        raise ValueError('INHERITED_FILE_SET')
    # B/C Validate calls remain mandatory for genesis/guard/profile semantics.
    for path, digest in m['files_sha256'].items():
        if file_digest(bundle, 'files/' + relative(path)) != digest:
            raise ValueError('BUNDLE_FILE_MISMATCH: ' + path)
    inventory = {}
    for name, path in expected_components.items():
        d = decode(bounded(bundle, 'files/' + path, MAX_DESCRIPTOR))
        if set(d) != {'head', 'tree', 'implementation_settings'} or any(not isinstance(d[k], str) or not re.fullmatch('[0-9a-f]{40}', d[k]) for k in ('head', 'tree')):
            raise ValueError('DESCRIPTOR_POLICY')
        settings = d['implementation_settings']
        if not isinstance(settings, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in settings.items()):
            raise ValueError('DESCRIPTOR_SETTINGS')
        hashes = decode(settings['artifacts_sha256_json'])
        if not isinstance(hashes, dict) or not hashes:
            raise ValueError('ARTIFACT_INVENTORY_REQUIRED')
        for artifact, digest in hashes.items():
            if not isinstance(digest, str) or not re.fullmatch('[0-9a-f]{64}', digest) or file_digest(artifacts, artifact) != digest:
                raise ValueError('ARTIFACT_MISMATCH: ' + artifact)
            if artifact in inventory and inventory[artifact] != digest:
                raise ValueError('ARTIFACT_CONFLICT')
            inventory[artifact] = digest
    return {'format': 's3-local-byte-preflight/1', 'manifest_sha256': pin,
            'artifact_count': len(inventory), 'byte_match': True,
            'approval_verified': False, 'services_started': False,
            'durable_ack': False, 'DEV': 'NOT_RUN'}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bundle', type=Path, required=True)
    p.add_argument('--artifacts', type=Path, required=True)
    p.add_argument('--runtime-pin', required=True)
    p.add_argument('--local-demo-profile', required=True)
    p.add_argument('--acknowledge-unproven-space', action='store_true')
    a = p.parse_args()
    try:
        print(json.dumps(verify(a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile, a.acknowledge_unproven_space), sort_keys=True))
    except (ValueError, OSError, KeyError, TypeError) as e:
        p.exit(2, 'PREFLIGHT_REJECTED: ' + str(e) + '\n')


if __name__ == '__main__':
    main()
