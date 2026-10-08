import base64
import hashlib
from pathlib import Path
import unittest
from test_component_sources import Inclusion
import source_reconciliation as m


class Reconciliation(unittest.TestCase):
    setUp = Inclusion.setUp
    git = Inclusion.git
    commit = Inclusion.commit
    def branch_pair(self, left, right):
        (self.root/'web/a.ts').write_text('first\nmiddle\nlast\n')
        base = self.commit()
        (self.root/'web/a.ts').write_text(left)
        l = self.commit()
        self.git('checkout', '-q', base)
        (self.root/'web/a.ts').write_text(right)
        return l, self.commit()

    def test_clean_merge_is_exact_evidence_not_permission(self):
        l, r = self.branch_pair('LEFT\nmiddle\nlast\n', 'first\nmiddle\nRIGHT\n')
        result = m.reconcile(self.root, l, r, 'web/', scratch=self.root)
        row = result['files']['web/a.ts']
        raw = base64.b64decode(row['merged_bytes_base64'])
        self.assertEqual(raw, b'LEFT\nmiddle\nRIGHT\n')
        self.assertEqual(row['result']['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(row['kind'], 'clean_text_merge_requires_review')
        self.assertFalse(result['sealing_exception_granted'])
        self.assertFalse(result['approval_verified'])
        self.assertEqual(set(result['trees']), {l, r, result['base_head']})
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ['.git', 'web'])

    def test_conflict_has_no_usable_result(self):
        l, r = self.branch_pair('LEFT\nmiddle\nlast\n', 'RIGHT\nmiddle\nlast\n')
        row = m.reconcile(self.root, l, r, 'web/', scratch=self.root)['files']['web/a.ts']
        self.assertEqual(row['kind'], 'manual_resolution_required')
        self.assertIsNone(row['result'])
        self.assertNotIn('merged_bytes_base64', row)

    def test_mode_conflict_and_exact_head_rejection(self):
        l, r = self.branch_pair('LEFT\nmiddle\nlast\n', 'first\nmiddle\nRIGHT\n')
        (self.root/'web/a.ts').chmod(0o755)
        r = self.commit()
        row = m.reconcile(self.root, l, r, 'web/', scratch=self.root)['files']['web/a.ts']
        self.assertIsNone(row['result'])
        with self.assertRaisesRegex(ValueError, 'EXACT_COMMIT'):
            m.reconcile(self.root, 'HEAD', r, 'web/', scratch=self.root)


if __name__ == '__main__':
    suite = unittest.TestSuite(Reconciliation(name) for name in (
        'test_clean_merge_is_exact_evidence_not_permission',
        'test_conflict_has_no_usable_result', 'test_mode_conflict_and_exact_head_rejection'))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise SystemExit(not result.wasSuccessful())
