"""Actual Git contract mutation tests; no services, pin or approval issued."""
from pathlib import Path
from unittest.mock import patch
import unittest
import manifest
import test_component_sources


class ContractGate(test_component_sources.Inclusion):
    def setUp(self):
        super().setUp()
        for prefix in ('protocol/s3/', manifest.PREFIX, manifest.PUBLIC_PREFIX):
            path = self.root / prefix
            path.mkdir(parents=True)
            (path/'contract.json').write_bytes(b'{"approved":true}\n')
        self.approved = self.commit()

    def inspect(self, head):
        with patch.object(manifest, 'A', self.approved):
            return manifest.contract_identity(self.root, head)

    def test_contract_unchanged_with_implementation_lock_change(self):
        (self.root/'Cargo.lock').write_text('new implementation lock')
        candidate = self.commit()
        report = self.inspect(candidate)
        self.assertEqual(set(report), {'protocol/s3/', manifest.PREFIX, manifest.PUBLIC_PREFIX})
        self.assertTrue(all(x['approved_files_preserved'] for x in report.values()))
        # Evidence is pinned to commit; working tree changes are handled by source_identity.
        (self.root/'protocol/s3/contract.json').write_bytes(b'dirty')
        self.assertEqual(report, self.inspect(candidate))

    def test_contract_changes_require_review_despite_ancestry(self):
        for prefix in ('protocol/s3/', manifest.PREFIX, manifest.PUBLIC_PREFIX):
            for change in ('bytes', 'mode', 'missing', 'added'):
                with self.subTest(prefix=prefix, change=change):
                    self.git('reset', '--hard', self.approved)
                    path = self.root/prefix/'contract.json'
                    if change == 'bytes': path.write_bytes(b'changed')
                    elif change == 'mode': path.chmod(0o755)
                    elif change == 'missing':
                        path.unlink()
                        (path.parent/'other').write_bytes(b'new')
                    else: (path.parent/'extra').write_bytes(b'new')
                    candidate = self.commit()
                    self.git('merge-base', '--is-ancestor', self.approved, candidate)
                    with self.assertRaisesRegex(ValueError, 'CONTRACT_SOURCE_REVIEW_REQUIRED'):
                        self.inspect(candidate)

if __name__ == '__main__': unittest.main()
