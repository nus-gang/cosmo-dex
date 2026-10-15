import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import component_sources as m


class Inclusion(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = dict(os.environ, GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid',
                        GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid')
        self.git('init', '-q')
        (self.root/'web').mkdir()
        (self.root/'web/a.ts').write_text('approved bytes\n')
        self.source = self.commit()

    def git(self, *args):
        return subprocess.check_output(['/usr/bin/git', '-C', str(self.root), *args], env=self.env, stderr=subprocess.PIPE)

    def commit(self):
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture')
        return self.git('rev-parse', 'HEAD').decode().strip()

    def test_ancestor_with_changed_bytes_fails(self):
        (self.root/'web/a.ts').write_text('unreviewed replacement\n')
        candidate = self.commit()
        self.git('merge-base', '--is-ancestor', self.source, candidate)
        report = m.compare(self.root, self.source, candidate, 'web/')
        self.assertFalse(report['approved_files_preserved'])
        self.assertEqual(list(report['changed']), ['web/a.ts'])

    def test_missing_and_mode_change_fail(self):
        (self.root/'web/a.ts').chmod(0o755)
        report = m.compare(self.root, self.source, self.commit(), 'web/')
        self.assertFalse(report['approved_files_preserved'])
        self.assertEqual(report['changed']['web/a.ts']['candidate']['mode'], '100755')
        (self.root/'web/a.ts').unlink()
        (self.root/'web/other.ts').write_text('new')
        report = m.compare(self.root, self.source, self.commit(), 'web/')
        self.assertEqual(report['missing'], ['web/a.ts'])
        self.assertFalse(report['approved_files_preserved'])

    def test_additions_are_separate_and_worktree_is_not_claimed(self):
        (self.root/'web/launcher.ts').write_text('sre addition')
        candidate = self.commit()
        (self.root/'web/a.ts').write_text('dirty worktree ignored explicitly')
        report = m.compare(self.root, self.source, candidate, 'web/')
        self.assertTrue(report['approved_files_preserved'])
        self.assertTrue(report['additions_require_review'])
        self.assertEqual(list(report['added']), ['web/launcher.ts'])
        self.assertEqual(report['source_head'], self.source)

    def test_exact_revision_and_object_rules(self):
        for revision in ('HEAD', self.source[:7], '-bad'):
            with self.assertRaisesRegex(ValueError, 'EXACT_COMMIT'):
                m.entries(self.root, revision, 'web/')
        for prefix in ('../', '/web/', 'web', 'web//'):
            with self.assertRaisesRegex(ValueError, 'PREFIX'):
                m.entries(self.root, self.source, prefix)
        (self.root/'web/link').symlink_to('a.ts')
        with self.assertRaisesRegex(ValueError, 'UNSUPPORTED'):
            m.entries(self.root, self.commit(), 'web/')


if __name__ == '__main__': unittest.main()
