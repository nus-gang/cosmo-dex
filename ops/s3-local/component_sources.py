#!/usr/bin/env python3
"""Offline component inclusion evidence; does not certify approval or seal a runtime."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

CANDIDATES = {
    'chain': ('497ecba3008de9168c431facc4ff9fc8a4fc329b', 'chain/'),
    'exchange': ('20c0cd9af0eb305341ee5c7e058350389f03a2b3', 'exchange/'),
    'settlement': ('46546d317701b127da8196e9e6abdab7ce9a3d6e', 'settlement/'),
    'wallet': ('720163e80cf239279d49ce58304fd3865e6bd684', 'web/'),
}


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], stderr=subprocess.PIPE)


def entries(root, revision, prefix):
    if not re.fullmatch('[0-9a-f]{40}', revision):
        raise ValueError('EXACT_COMMIT_REQUIRED')
    if not prefix.endswith('/') or prefix.startswith('/') or any(x in ('', '.', '..') for x in prefix[:-1].split('/')):
        raise ValueError('COMPONENT_PREFIX_INVALID')
    if git(root, 'rev-parse', revision+'^{commit}').decode().strip() != revision:
        raise ValueError('EXACT_COMMIT_REQUIRED')
    result = {}
    objects = []
    for record in git(root, 'ls-tree', '-rz', revision, '--', prefix).split(b'\0'):
        if not record:
            continue
        meta, raw_path = record.split(b'\t', 1)
        mode, kind, oid = meta.decode('ascii').split()
        path = raw_path.decode('utf-8')
        if kind != 'blob' or mode not in ('100644', '100755') or not path.startswith(prefix):
            raise ValueError('UNSUPPORTED_COMPONENT_OBJECT')
        objects.append((path, mode, oid))
    if not objects:
        raise ValueError('EMPTY_COMPONENT')
    # One Git process per tree; avoid invoking a credential wrapper per file.
    stream = subprocess.check_output(['git', '-C', str(root), 'cat-file', '--batch'],
                                    input=''.join(oid+'\n' for _, _, oid in objects).encode(),
                                    stderr=subprocess.PIPE)
    cursor = 0
    for path, mode, oid in objects:
        end = stream.index(b'\n', cursor)
        got_oid, kind, size = stream[cursor:end].decode('ascii').split()
        size = int(size)
        if got_oid != oid or kind != 'blob' or size < 0:
            raise ValueError('COMPONENT_BLOB_INVALID')
        cursor = end + 1
        raw = stream[cursor:cursor+size]
        if len(raw) != size or stream[cursor+size:cursor+size+1] != b'\n':
            raise ValueError('COMPONENT_BLOB_INVALID')
        result[path] = dict(mode=mode, sha256=hashlib.sha256(raw).hexdigest())
        cursor += size + 1
    if cursor != len(stream):
        raise ValueError('COMPONENT_BLOB_INVALID')
    return result


def compare(root, source_commit, candidate_commit, prefix):
    source = entries(root, source_commit, prefix)
    candidate = entries(root, candidate_commit, prefix)
    missing = sorted(set(source) - set(candidate))
    changed = {p: dict(source=source[p], candidate=candidate[p]) for p in sorted(set(source) & set(candidate)) if source[p] != candidate[p]}
    added = {p: candidate[p] for p in sorted(set(candidate) - set(source))}
    return dict(source_head=source_commit,
                source_tree=git(root, 'rev-parse', source_commit+'^{tree}').decode().strip(),
                prefix=prefix, source_files=source, missing=missing, changed=changed, added=added,
                approved_files_preserved=not missing and not changed,
                additions_require_review=bool(added))


def audit(root, candidate_commit):
    components = {name: compare(root, head, candidate_commit, prefix)
                  for name, (head, prefix) in CANDIDATES.items()}
    return dict(schema='s3-local-component-inclusion/1', candidate_head=candidate_commit,
                candidate_tree=git(root, 'rev-parse', candidate_commit+'^{tree}').decode().strip(),
                components=components, approved_files_preserved=all(c['approved_files_preserved'] for c in components.values()),
                working_tree_checked=False, approval_verified=False, runtime_approved=False,
                DEV='NOT_RUN')


def main():
    p = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    p.add_argument('--source', type=Path, required=True)
    p.add_argument('--candidate', required=True)
    a = p.parse_args()
    report = audit(a.source, a.candidate)
    print(json.dumps(report, sort_keys=True, indent=2))
    return 0 if report['approved_files_preserved'] else 1


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError):
        raise SystemExit('COMPONENT_INCLUSION_INVALID')
