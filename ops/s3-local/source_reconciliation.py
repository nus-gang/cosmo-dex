#!/usr/bin/env python3
"""Produce exact overlapping-source review evidence; never grant a sealing exception."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import component_sources as sources


def compare_candidate(root, left, right, prefix, candidate, *, scratch):
    """Recompute provenance from Git; never trust a caller-supplied merge report."""
    actual = sources.entries(root, candidate, prefix)
    provenance = reconcile(root, left, right, prefix, scratch=scratch)
    unresolved = sorted(p for p, row in provenance['files'].items()
                        if row['kind'] == 'manual_resolution_required')
    # A resolved deletion has result=None, but is not an unresolved conflict.
    expected = {p: row['result'] for p, row in provenance['files'].items()
                if row['result'] is not None}
    missing = sorted(set(expected) - set(actual))
    changed = {p: dict(expected=expected[p], candidate=actual[p])
               for p in sorted(set(expected) & set(actual)) if expected[p] != actual[p]}
    added = {p: actual[p] for p in sorted(set(actual) - set(expected))}
    raw = (json.dumps(provenance, sort_keys=True, separators=(',', ':'))+'\n').encode()
    return dict(schema='s3-local-reconciled-candidate/1',
                candidate_head=candidate,
                candidate_tree=sources.git(root, 'rev-parse', candidate+'^{tree}').decode().strip(),
                reconciliation=provenance,
                reconciliation_sha256=hashlib.sha256(raw).hexdigest(),
                missing=missing, changed=changed, added=added, unresolved=unresolved,
                exact_reconciled_tree=not (missing or changed or added or unresolved),
                working_tree_checked=False, approval_verified=False,
                sealing_exception_granted=False, runtime_approved=False)


def reconcile(root, left, right, prefix, *, scratch):
    left_files = sources.entries(root, left, prefix)
    right_files = sources.entries(root, right, prefix)
    bases = sources.git(root, 'merge-base', '--all', left, right).decode().splitlines()
    if len(bases) != 1:
        raise ValueError('UNIQUE_MERGE_BASE_REQUIRED')
    base = bases[0]
    base_files = sources.entries(root, base, prefix)
    rows = {}
    for path in sorted(set(left_files) | set(right_files)):
        l, r, b = left_files.get(path), right_files.get(path), base_files.get(path)
        row = dict(left=l, right=r, base=b)
        if l == r:
            row.update(kind='identical', result=l)
        elif l == b:
            row.update(kind='right_change', result=r)
        elif r == b:
            row.update(kind='left_change', result=l)
        elif l is None or r is None or b is None or len({l['mode'], r['mode'], b['mode']}) != 1:
            row.update(kind='manual_resolution_required', result=None)
        else:
            with tempfile.TemporaryDirectory(dir=scratch) as temp:
                paths = []
                for name, head in [('left', left), ('base', base), ('right', right)]:
                    p = Path(temp)/name
                    p.write_bytes(sources.git(root, 'show', head+':'+path))
                    paths.append(str(p))
                merged = subprocess.run(['git', 'merge-file', '-p', *paths], capture_output=True)
            if merged.returncode == 0:
                row.update(kind='clean_text_merge_requires_review', result=dict(
                    mode=l['mode'], sha256=hashlib.sha256(merged.stdout).hexdigest()),
                    merged_bytes_base64=__import__('base64').b64encode(merged.stdout).decode())
            elif merged.returncode == 1:
                row.update(kind='manual_resolution_required', result=None)
            else:
                raise ValueError('MERGE_TOOL_FAILED')
        rows[path] = row
    return dict(schema='s3-local-source-reconciliation/1', prefix=prefix,
                left_head=left, right_head=right, base_head=base,
                trees={head: sources.git(root, 'rev-parse', head+'^{tree}').decode().strip()
                       for head in (left, right, base)}, files=rows,
                merge_tool=sources.git(root, '--version').decode().strip(),
                approval_verified=False, sealing_exception_granted=False,
                runtime_approved=False)


def main(argv=None):
    import argparse
    import sys
    class Parser(argparse.ArgumentParser):
        def error(self, message):
            raise ValueError('INVALID_ARGUMENTS')
    p = Parser(description=__doc__, allow_abbrev=False)
    for name in ('source', 'left', 'right', 'prefix', 'candidate', 'scratch'):
        p.add_argument('--'+name, required=True, action='append')
    try:
        args = p.parse_args(argv)
        values = vars(args)
        if any(len(value) != 1 for value in values.values()):
            raise ValueError('DUPLICATE_ARGUMENT')
        values = {key: value[0] for key, value in values.items()}
        scratch = Path(values['scratch'])
        if not scratch.is_absolute() or scratch.is_symlink() or not scratch.is_dir():
            raise ValueError('SCRATCH_REQUIRED')
        report = compare_candidate(Path(values['source']), values['left'], values['right'],
                                   values['prefix'], values['candidate'], scratch=scratch)
        raw = json.dumps(report, sort_keys=True, separators=(',', ':'))+'\n'
    except (ValueError, OSError, subprocess.CalledProcessError, UnicodeError):
        print('SOURCE_RECONCILIATION_INVALID', file=sys.stderr)
        return 2
    sys.stdout.write(raw)
    return 0 if report['exact_reconciled_tree'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
