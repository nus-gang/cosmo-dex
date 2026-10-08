"""Binding regression tests. All executable bytes are explicitly synthetic test fixtures."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('manifest', Path(__file__).with_name('manifest.py'))
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)
ROOT = Path(__file__).resolve().parents[2]


class Binding(unittest.TestCase):
    def test_approved_originals_and_distinct_implementation_lock(self):
        files = m.inherited(ROOT)
        self.assertEqual(len(files), 274)
        self.assertEqual(m.sha(files['protocol/s3/manifest.json']), m.BASELINE)
        self.assertNotEqual(files['chain/app/go.mod'], (ROOT/'chain/app/go.mod').read_bytes())
        self.assertNotIn('runtime-manifest.json', files)
        self.assertNotIn('runtime/genesis.json', files)  # Approved historical fixtures remain inherited.
        self.assertEqual(m.sha(files[m.PUBLIC_PREFIX+'MANIFEST.json']), m.PUBLIC_MANIFEST)
        self.assertEqual(m.sha(files[m.PUBLIC_PREFIX+'schema.json']), m.PUBLIC_SCHEMA)

    def test_reject_duplicate_json_and_unsafe_paths(self):
        with self.assertRaisesRegex(ValueError, 'DUPLICATE'):
            m.decode(b'{"head":"a","head":"b"}')
        for path in ('../outside', '/absolute', 'a//b', 'a/./b', 'a\\b', 'a\nb'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                m.relative(path)

    def test_no_overwrite_or_link_consumption(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root/'real').write_bytes(b'fixture')
            (root/'alias').symlink_to('real')
            with self.assertRaisesRegex(ValueError, 'SYMLINK'):
                m.read_regular(root, 'alias')
            m.write_new(root/'out', {'audit.json': b'original'})
            with self.assertRaises(FileExistsError):
                m.write_new(root/'out', {'audit.json': b'replacement'})
            self.assertEqual((root/'out/audit.json').read_bytes(), b'original')

    @patch.object(m, 'audit', side_effect=lambda root: (m.inherited(root), {'head': 'a'*40, 'tree': 'b'*40, 'locks': {}, 'runtime_approved': False}))
    def test_binary_edit_changes_descriptor_aggregate_and_runtime_hash(self, _identity):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root/'synthetic-test-only').write_bytes(b'NOT_A_RUNTIME_BINARY_1')
            spec = {name: {'build_argv': ['fixture-only'], 'toolchain': 'fixture-only',
                          'artifacts': ['synthetic-test-only'], 'approval_sources': ['TEST_ONLY_NOT_APPROVED'],
                          'settings': {'fixture': 'true'}} for name in m.COMPONENTS}
            files, report, first = m.make_candidate(ROOT, root, spec)
            self.assertEqual(len(files), 279)
            self.assertFalse(report['runtime_approved'])
            self.assertNotIn('approved_runtime_sha256', report)
            self.assertEqual(set(first['components']), set(m.COMPONENTS))
            (root/'synthetic-test-only').write_bytes(b'NOT_A_RUNTIME_BINARY_2')
            _, _, second = m.make_candidate(ROOT, root, spec)
            self.assertNotEqual(first['contract_sha256'], second['contract_sha256'])
            self.assertNotEqual(m.sha(m.encode(first)), m.sha(m.encode(second)))
            (root/'synthetic-test-only').unlink()
            with self.assertRaisesRegex(ValueError, 'NOT_SINGLE_REGULAR_FILE'):
                m.make_candidate(ROOT, root, spec)


class SourceGate(unittest.TestCase):
    def identity(self, report, dirty=False):
        def git(_root, *args):
            if args == ('status', '--porcelain'):
                return b' M file' if dirty else b''
            if args[0] == 'merge-base':
                return b''
            return (('b' if args[-1].endswith('^{tree}') else 'a')*40+'\n').encode()
        with patch.object(m, 'contract_identity', return_value={'fixture': True}), patch.object(m, 'git', side_effect=git), patch.object(m.component_sources, 'audit', return_value=report) as audit:
            result = m.source_identity(ROOT)
            audit.assert_called_once_with(ROOT, 'a'*40)
            return result

    def test_changed_or_missing_source_blocks_even_with_all_ancestors(self):
        for kind in ('changed', 'missing'):
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, 'COMPONENT_SOURCE_RECONCILIATION_REQUIRED'):
                self.identity({'approved_files_preserved': False, kind: ['exchange/file']})

    def test_clean_inclusion_is_evidence_not_runtime_approval(self):
        report = {'approved_files_preserved': True, 'approval_verified': False,
                  'runtime_approved': False, 'additions_require_review': True}
        result = self.identity(report)
        self.assertEqual(result['component_inclusion'], report)
        self.assertEqual(m.HEADS['exchange'], m.component_sources.CANDIDATES['exchange'][0])
        self.assertEqual(m.HEADS['wallet'], m.component_sources.CANDIDATES['wallet'][0])
        self.assertEqual(m.ANCESTORS['exchange_contract'], '2bb2f6d29da23e1479be85bc16cb175cdb525367')

    def test_dirty_source_rejected_before_inclusion(self):
        with self.assertRaisesRegex(ValueError, 'DIRTY_SOURCE'):
            self.identity({}, dirty=True)


if __name__ == '__main__':
    unittest.main()
