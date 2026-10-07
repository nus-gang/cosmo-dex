"""Synthetic audit/artifact races; no runtime approval or service execution."""
import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import manifest as m


class SealConsistency(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'binary').write_bytes(b'synthetic binary')
        self.files = {'contract': b'approved fixture'}
        self.report = {'head': 'a'*40, 'tree': 'b'*40, 'locks': {},
                       'runtime_approved': False}
        self.spec = {name: {'build_argv': ['fixture'], 'toolchain': 'fixture',
                           'artifacts': ['binary'], 'approval_sources': ['fixture'],
                           'settings': {}} for name in m.COMPONENTS}

    def audit(self):
        return copy.deepcopy((self.files, self.report))

    def test_unchanged_seal_rechecks_source_and_keeps_approval_false(self):
        with patch.object(m, 'audit', side_effect=lambda _: self.audit()) as audit:
            _, report, manifest = m.make_candidate(self.root, self.root, self.spec)
        self.assertEqual(audit.call_count, 2)
        self.assertFalse(report['runtime_approved'])
        self.assertEqual(report['candidate_runtime_manifest_sha256'], m.sha(m.encode(manifest)))

    def test_final_source_change_or_dirty_state_returns_no_candidate(self):
        for changed in ('head', 'tree', 'locks', 'contract', 'dirty'):
            before = self.audit()
            after = self.audit()
            if changed == 'contract':
                after[0]['contract'] = b'changed'
            elif changed == 'dirty':
                after = ValueError('DIRTY_SOURCE')
            else:
                after[1][changed] = {'changed': True} if changed == 'locks' else 'c'*40
            with self.subTest(changed=changed), patch.object(m, 'audit', side_effect=[before, after]):
                with self.assertRaisesRegex(ValueError, 'SOURCE_CHANGED_DURING_SEAL|DIRTY_SOURCE'):
                    m.make_candidate(self.root, self.root, self.spec)

    def test_artifact_change_between_components_or_final_read_is_rejected(self):
        for changed_at in (2, 6):
            calls = 0
            original = m.read_regular
            def read(root, path):
                nonlocal calls
                calls += 1
                if calls == changed_at:
                    (root / path).write_bytes(b'changed binary')
                return original(root, path)
            (self.root / 'binary').write_bytes(b'synthetic binary')
            with self.subTest(changed_at=changed_at), patch.object(m, 'audit', side_effect=lambda _: self.audit()), patch.object(m, 'read_regular', side_effect=read):
                with self.assertRaisesRegex(ValueError, 'BUILD_ARTIFACT_CHANGED'):
                    m.make_candidate(self.root, self.root, self.spec)

    def test_artifact_link_or_removal_before_final_read_is_rejected(self):
        for change in ('link', 'remove'):
            (self.root / 'binary').write_bytes(b'synthetic binary')
            calls = 0
            original = m.read_regular
            def read(root, path):
                nonlocal calls
                calls += 1
                if calls == 6:
                    (root / path).unlink()
                    if change == 'link':
                        (root / 'other').write_bytes(b'synthetic binary')
                        (root / path).symlink_to('other')
                return original(root, path)
            with self.subTest(change=change), patch.object(m, 'audit', side_effect=lambda _: self.audit()), patch.object(m, 'read_regular', side_effect=read):
                with self.assertRaisesRegex(ValueError, 'SYMLINK|NOT_SINGLE_REGULAR_FILE'):
                    m.make_candidate(self.root, self.root, self.spec)
            (self.root / 'binary').unlink(missing_ok=True)

if __name__ == '__main__':
    unittest.main()
