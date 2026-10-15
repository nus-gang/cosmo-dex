import hashlib
import json
import unittest
import test_source_reconciliation as fixture
import source_reconciliation as m


class Candidate(unittest.TestCase):
    setUp = fixture.Reconciliation.setUp
    git = fixture.Reconciliation.git
    commit = fixture.Reconciliation.commit
    branch_pair = fixture.Reconciliation.branch_pair

    def report(self, l, r, candidate):
        return m.compare_candidate(self.root, l, r, 'web/', candidate, scratch=self.root)

    def test_exact_candidate_and_dirty_worktree_are_distinct(self):
        l, r = self.branch_pair('LEFT\nmiddle\nlast\n', 'first\nmiddle\nRIGHT\n')
        (self.root/'web/a.ts').write_text('LEFT\nmiddle\nRIGHT\n')
        head = self.commit()
        (self.root/'web/a.ts').write_text('uncommitted change')
        result = self.report(l, r, head)
        self.assertTrue(result['exact_reconciled_tree'])
        for key in ('working_tree_checked', 'approval_verified', 'sealing_exception_granted', 'runtime_approved'):
            self.assertFalse(result[key])
        raw = (json.dumps(result['reconciliation'], sort_keys=True, separators=(',', ':'))+'\n').encode()
        self.assertEqual(result['reconciliation_sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual(result['candidate_head'], head)

    def test_content_mode_missing_and_addition_are_reported(self):
        l, r = self.branch_pair('LEFT\nmiddle\nlast\n', 'first\nmiddle\nRIGHT\n')
        result = self.report(l, r, r)
        self.assertEqual(list(result['changed']), ['web/a.ts'])
        (self.root/'web/a.ts').write_text('LEFT\nmiddle\nRIGHT\n')
        (self.root/'web/a.ts').chmod(0o755)
        result = self.report(l, r, self.commit())
        self.assertEqual(result['changed']['web/a.ts']['candidate']['mode'], '100755')
        (self.root/'web/a.ts').unlink()
        (self.root/'web/extra.ts').write_text('extra')
        result = self.report(l, r, self.commit())
        self.assertEqual(result['missing'], ['web/a.ts'])
        self.assertEqual(list(result['added']), ['web/extra.ts'])
        self.assertFalse(result['exact_reconciled_tree'])

    def test_conflict_is_not_resolved_by_candidate_and_refs_are_rejected(self):
        l, r = self.branch_pair('LEFT\nmiddle\nlast\n', 'RIGHT\nmiddle\nlast\n')
        result = self.report(l, r, r)
        self.assertEqual(result['unresolved'], ['web/a.ts'])
        self.assertFalse(result['exact_reconciled_tree'])
        with self.assertRaisesRegex(ValueError, 'EXACT_COMMIT_REQUIRED'):
            self.report(l, r, 'HEAD')
        (self.root/'web/link').symlink_to('a.ts')
        with self.assertRaisesRegex(ValueError, 'UNSUPPORTED_COMPONENT_OBJECT'):
            self.report(l, r, self.commit())

    def test_one_sided_deletion_is_resolved_not_conflicting(self):
        (self.root/'web/keep.ts').write_text('kept')
        base = self.commit()
        (self.root/'web/a.ts').unlink()
        left = self.commit()
        result = self.report(left, base, left)
        self.assertTrue(result['exact_reconciled_tree'])
        self.assertEqual(result['unresolved'], [])
        self.assertIsNone(result['reconciliation']['files']['web/a.ts']['result'])


if __name__ == '__main__': unittest.main()
