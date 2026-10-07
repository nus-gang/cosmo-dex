#!/usr/bin/env python3
"""Offline L-R source audit and runtime candidate sealer. Never approves or starts services."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import component_sources

A = 'fd9aa6ca9093817e4ab09d2ae835197a84bbade6'
CANDIDATE = '90169d322336a0c0de9bc6c48725d528d42fe74c78ea5b596fc7e059d747dda2'
BASELINE = '3ff69e73057a2bb6dcff64820123d520b9ad3e5637abbd1ad7d38b8c1a49eb97'
PREFIX = 'proposals/s3-local-dev-v1/'
HEADS = {
    'contract': A,
    **{name: source[0] for name, source in component_sources.CANDIDATES.items()},
}
COMPONENTS = ('chain', 'exchange', 'settlement', 'wallet', 'sre')
LOCKS = ('chain/app/go.mod', 'chain/app/go.sum', 'chain/go.mod', 'chain/go.sum',
         'exchange/Cargo.toml', 'exchange/Cargo.lock', 'rust-toolchain.toml',
         'web/package.json', 'web/package-lock.json')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encode(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False) + '\n').encode()


def unique(pairs):
    value = {}
    for k, v in pairs:
        if k in value:
            raise ValueError('DUPLICATE_JSON_KEY: ' + k)
        value[k] = v
    return value


def decode(raw):
    return json.loads(raw, object_pairs_hook=unique)


def relative(path):
    if not isinstance(path, str) or not path or '\\' in path or any(c in path for c in '\0\r\n'):
        raise ValueError('INVALID_PATH')
    p = PurePosixPath(path)
    if p.is_absolute() or any(c in ('', '.', '..') for c in path.split('/')):
        raise ValueError('INVALID_PATH: ' + path)
    return path


def aggregate(files):
    for p, h in files.items():
        relative(p)
        if not isinstance(h, str) or not re.fullmatch('[0-9a-f]{64}', h):
            raise ValueError('INVALID_SHA256')
    return sha(''.join(f'{files[p]}  {p}\n' for p in sorted(files)).encode())


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], stderr=subprocess.PIPE)


def read_regular(root, name):
    """Capture one bounded regular file through no-follow directory descriptors."""
    relative(name)
    # Keep the existing diagnostic for links/missing paths, but do not trust
    # those path checks to authorize the subsequent open.
    p = root
    for part in name.split('/'):
        p = p / part
        if p.is_symlink():
            raise ValueError('SYMLINK: ' + name)
    if not p.is_file():
        raise ValueError('NOT_SINGLE_REGULAR_FILE: ' + name)
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    item = None
    try:
        parts = name.split('/')
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=directory)
            os.close(directory)
            directory = child
        item = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                       dir_fd=directory)
        before = os.fstat(item)
        # Match the runtime byte-preflight artifact limit (512 MiB).
        if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or
                before.st_size > 536870912):
            raise ValueError('NOT_SINGLE_REGULAR_FILE: ' + name)
        chunks, count = [], 0
        while True:
            raw = os.read(item, min(1048576, before.st_size - count + 1))
            if not raw:
                break
            count += len(raw)
            if count > before.st_size:
                raise ValueError('FILE_CHANGED_DURING_READ: ' + name)
            chunks.append(raw)
        signature = lambda s: (s.st_dev, s.st_ino, s.st_mode, s.st_nlink,
                                s.st_size, s.st_mtime_ns, s.st_ctime_ns)
        after = os.fstat(item)
        current = os.stat(parts[-1], dir_fd=directory, follow_symlinks=False)
        if (count != before.st_size or signature(before) != signature(after) or
                signature(after) != signature(current)):
            raise ValueError('FILE_CHANGED_DURING_READ: ' + name)
        return b''.join(chunks)
    finally:
        if item is not None:
            os.close(item)
        os.close(directory)


def contract_identity(root, head):
    """The candidate must retain A's contract bytes/modes, not only its ancestry."""
    report = {prefix: component_sources.compare(root, A, head, prefix)
              for prefix in ('protocol/s3/', PREFIX)}
    if any(not item['approved_files_preserved'] or item['added']
           for item in report.values()):
        raise ValueError('CONTRACT_SOURCE_REVIEW_REQUIRED')
    return report


def source_identity(root):
    head = git(root, 'rev-parse', 'HEAD').decode().strip()
    tree = git(root, 'rev-parse', 'HEAD^{tree}').decode().strip()
    if git(root, 'status', '--porcelain'):
        raise ValueError('DIRTY_SOURCE')
    approvals = {}
    for name, h in HEADS.items():
        git(root, 'merge-base', '--is-ancestor', h, head)
        approvals[name] = {'head': h, 'tree': git(root, 'rev-parse', h+'^{tree}').decode().strip()}
    contract = contract_identity(root, head)
    inclusion = component_sources.audit(root, head)
    if not inclusion['approved_files_preserved']:
        raise ValueError('COMPONENT_SOURCE_RECONCILIATION_REQUIRED')
    return {'head': head, 'tree': tree, 'approved_ancestors': approvals,
            'component_inclusion': inclusion, 'contract_inclusion': contract}


def inherited(root):
    """Read approved snapshot from Git A; never reseal the current implementation lock."""
    get = lambda path: git(root, 'show', A+':'+relative(path))
    baseline = get('protocol/s3/manifest.json')
    candidate = get(PREFIX+'MANIFEST.json')
    if sha(baseline) != BASELINE or sha(candidate) != CANDIDATE:
        raise ValueError('APPROVED_MANIFEST_MISMATCH')
    b, c = decode(baseline), decode(candidate)
    if aggregate(b['files_sha256']) != b['contract_sha256']:
        raise ValueError('RC3_AGGREGATE_MISMATCH')
    expected = dict(b['files_sha256'])
    expected.update({PREFIX+p: h for p, h in c['files_sha256'].items()})
    expected.update({'protocol/s3/manifest.json': BASELINE, PREFIX+'MANIFEST.json': CANDIDATE})
    files = {}
    for path, digest in expected.items():
        raw = get(path)
        if sha(raw) != digest:
            raise ValueError('APPROVED_FILE_MISMATCH: ' + path)
        files[path] = raw
    return files


def audit(root):
    identity = source_identity(root)
    files = inherited(root)
    locks = {}
    for path in LOCKS:
        raw = read_regular(root, path)
        # Tracked bytes must match the declared implementation head.
        if raw != git(root, 'show', identity['head']+':'+path):
            raise ValueError('IMPLEMENTATION_FILE_MISMATCH: '+path)
        locks[path] = {'implementation_sha256': sha(raw),
                       'rc3_snapshot_sha256': sha(files[path]) if path in files else None}
    return files, {'format': 's3-local-source-audit/1', **identity, 'locks': locks,
                   'inherited_file_count': len(files), 'inherited_sha256': {p: sha(b) for p, b in files.items()},
                   'runtime_approved': False, 'services_started': False, 'DEV': 'NOT_RUN'}


def make_candidate(root, artifacts, spec):
    files, report = audit(root)
    source_report_raw = encode(report)
    inherited_hashes = {p: sha(raw) for p, raw in files.items()}
    if set(spec) != set(COMPONENTS):
        raise ValueError('EXACTLY_FIVE_COMPONENTS_REQUIRED')
    components = {}
    inventory = {}
    for name in COMPONENTS:
        s = spec[name]
        if set(s) != {'build_argv', 'toolchain', 'artifacts', 'approval_sources', 'settings'}:
            raise ValueError('COMPONENT_SPEC_FIELDS: '+name)
        if not isinstance(s['build_argv'], list) or not s['build_argv'] or not all(isinstance(a, str) and a for a in s['build_argv']):
            raise ValueError('BUILD_ARGV_REQUIRED')
        if not isinstance(s['toolchain'], str) or not s['toolchain']:
            raise ValueError('TOOLCHAIN_REQUIRED')
        if not isinstance(s['artifacts'], list) or not s['artifacts']:
            raise ValueError('REAL_BUILD_ARTIFACTS_REQUIRED: '+name)
        if not isinstance(s['approval_sources'], list) or not s['approval_sources'] or not all(isinstance(a, str) and a for a in s['approval_sources']):
            raise ValueError('APPROVAL_SOURCES_REQUIRED')
        if not isinstance(s['settings'], dict) or not all(isinstance(k, str) and isinstance(v, str) for k,v in s['settings'].items()):
            raise ValueError('STRING_SETTINGS_REQUIRED')
        hashes = {}
        for path in s['artifacts']:
            raw = read_regular(artifacts, path)
            if not raw:
                raise ValueError('EMPTY_ARTIFACT: '+path)
            hashes[path] = sha(raw)
        if len(hashes) != len(s['artifacts']):
            raise ValueError('DUPLICATE_ARTIFACT')
        inventory[name] = hashes
        settings = {
            'component_settings_json': encode(s['settings']).decode().strip(),
            'build_argv_json': encode(s['build_argv']).decode().strip(),
            'toolchain': s['toolchain'],
            'artifacts_sha256_json': encode(hashes).decode().strip(),
            'approval_sources_json': encode(s['approval_sources']).decode().strip(),
            'implementation_locks_json': encode(report['locks']).decode().strip(),
            'runtime_authorization': 'PENDING_INDEPENDENT_CEO_CTO_AND_CTO_SECURITY',
        }
        if name in HEADS:
            settings['component_approved_head'] = HEADS[name]
        path = 'chain/local-demo/components/'+name+'.json'
        components[name] = path
        files[path] = encode({'head': report['head'], 'tree': report['tree'], 'implementation_settings': settings})
    hashes = {p: sha(b) for p, b in files.items()}
    manifest = {'format': 's3-dev-local-runtime/1', 'scope': 'REVIEWED_RUNTIME',
                'candidate_manifest_sha256': CANDIDATE, 'contract_sha256': aggregate(hashes),
                'files_sha256': hashes, 'components': components}
    # Re-read at the end: a candidate must not combine different source or build
    # observations. This is a bounded consistency check, not a filesystem lock.
    for hashes in inventory.values():
        for path, digest in hashes.items():
            if sha(read_regular(artifacts, path)) != digest:
                raise ValueError('BUILD_ARTIFACT_CHANGED: '+path)
    final_files, final_report = audit(root)
    if (encode(final_report) != source_report_raw or
            {p: sha(raw) for p, raw in final_files.items()} != inherited_hashes):
        raise ValueError('SOURCE_CHANGED_DURING_SEAL')
    report['artifact_inventory'] = inventory
    report['candidate_runtime_manifest_sha256'] = sha(encode(manifest))
    report['authorization_note'] = 'Codec scope is not an approval. Do not use this hash as approved_runtime_sha256 before independent approval.'
    return files, report, manifest


def write_new(out, files):
    out.mkdir(mode=0o700, parents=False, exist_ok=False)
    for path, raw in files.items():
        target = out / relative(path)
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with target.open('xb') as f:
            f.write(raw)
    # This is a build artifact, never a genesis/home/guard creation operation.


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('mode', choices=['audit', 'seal'])
    p.add_argument('--source', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--build-spec', type=Path)
    p.add_argument('--artifacts', type=Path)
    a = p.parse_args()
    if a.mode == 'seal':
        if not a.build_spec or not a.artifacts:
            p.error('seal requires --build-spec and --artifacts')
        files, report, manifest = make_candidate(a.source.resolve(), a.artifacts.resolve(), decode(a.build_spec.read_bytes()))
        output = {'files/'+k: b for k,b in files.items()}
        output['runtime-manifest.json'] = encode(manifest)
    else:
        files, report = audit(a.source.resolve())
        output = {'files/'+k: b for k,b in files.items()}
    output['audit.json'] = encode(report)
    write_new(a.out, output)
    print(json.dumps({'output': str(a.out), 'inherited_file_count': report['inherited_file_count'], 'runtime_approved': False}))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as e:
        raise SystemExit(str(e))
