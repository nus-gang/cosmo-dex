"""Exercise the real manifest checker against isolated source mutations."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ManifestBoundary(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.getenv('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        # Only pinned inputs are needed; this also proves S1 is not required by S0.
        self.manifest = json.loads((ROOT / 'ops/ci/manifest.json').read_text())
        for name in [*self.manifest['files_sha256'], 'ops/ci/manifest.json']:
            dest = self.root / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, dest)

    def check(self, passes):
        result = subprocess.run([sys.executable, 'ops/ci/build_manifest.py', '--check'],
                                cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, passes, result.stderr)

    def test_baseline_without_s1(self):
        self.check(True)

    def test_s1_add_change_remove(self):
        app = self.root / 'chain/app'
        app.mkdir()
        for name in ('go.mod', 'new.go'):
            (app / name).write_text('synthetic S1 input\n')
        self.check(True)
        (app / 'new.go').write_text('changed S1 input\n')
        self.check(True)
        shutil.rmtree(app)
        self.check(True)

    def test_s0_source_changes_and_deletions_fail(self):
        for name in ('chain/contract/amount.go', 'chain/go.mod',
                     'chain/cmd/contract-runner/main.go', 'protocol/v1/schema.json',
                     'protocol/v1/vectors/s0-cases.json',
                     'protocol/v1/vectors/signatures.json',
                     'exchange/Cargo.toml', 'web/package.json',
                     'settlement/v1/fixtures.json', 'ops/ci/required-cases.json'):
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_bytes()
                path.write_bytes(original + b'\n')
                self.check(False)
                path.unlink()
                self.check(False)
                path.write_bytes(original)
        self.check(True)

    def test_unknown_nested_module_and_similar_prefix_fail(self):
        for folder in ('chain/other', 'chain/application', 'chain/contract/app'):
            with self.subTest(folder=folder):
                path = self.root / folder
                path.mkdir(parents=True)
                (path / 'go.mod').write_text('module synthetic\n')
                self.check(False)
                shutil.rmtree(path)

    def test_oracle_and_coverage_tampering_fail(self):
        path = self.root / 'ops/ci/manifest.json'
        for field in ('cases', 'coverage'):
            with self.subTest(field=field):
                changed = dict(self.manifest)
                changed[field] = self.manifest[field][1:]
                path.write_text(json.dumps(changed))
                self.check(False)
        path.write_text(json.dumps(self.manifest))
        self.check(True)


if __name__ == '__main__':
    unittest.main()
