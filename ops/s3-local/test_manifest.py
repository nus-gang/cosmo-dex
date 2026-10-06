"""Binding regression tests. All executable bytes are explicitly synthetic test fixtures."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('manifest', Path(__file__).with_name('manifest.py'))
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)
ROOT = Path(__file__).resolve().parents[2]


class Binding(unittest.TestCase):
    def test_approved_originals_and_distinct_implementation_lock(self):
        files = m.inherited(ROOT)
        self.assertEqual(len(files), 213)
        self.assertEqual(m.sha(files['protocol/s3/manifest.json']), m.BASELINE)
        self.assertNotEqual(files['chain/app/go.mod'], (ROOT/'chain/app/go.mod').read_bytes())
        self.assertNotIn('runtime-manifest.json', files)
        self.assertFalse(any('genesis' in p.split('/')[-1] and p.endswith('.json') for p in files))

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

    def test_binary_edit_changes_descriptor_aggregate_and_runtime_hash(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root/'synthetic-test-only').write_bytes(b'NOT_A_RUNTIME_BINARY_1')
            spec = {name: {'build_argv': ['fixture-only'], 'toolchain': 'fixture-only',
                          'artifacts': ['synthetic-test-only'], 'approval_sources': ['TEST_ONLY_NOT_APPROVED'],
                          'settings': {'fixture': 'true'}} for name in m.COMPONENTS}
            files, report, first = m.make_candidate(ROOT, root, spec)
            self.assertEqual(len(files), 218)
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


if __name__ == '__main__':
    unittest.main()
