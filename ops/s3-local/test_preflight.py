import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from manifest import COMPONENTS, CANDIDATE, aggregate, encode, inherited
from preflight import verify, file_digest, bounded

class PreflightTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.inherited = inherited(Path(__file__).resolve().parents[2])

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bundle = self.root / 'bundle'
        self.artifacts = self.root / 'artifacts'
        self.bundle.mkdir()
        self.artifacts.mkdir()
        self.binary = self.artifacts / 'TEST_ONLY_NOT_RUNTIME'
        self.binary.write_bytes(b'NOT_A_RUNTIME_BINARY')
        self.hashes = {}
        components = {}
        for name, raw in self.inherited.items():
            self.put(name, raw)
        for c in COMPONENTS:
            p = 'chain/local-demo/components/' + c + '.json'
            components[c] = p
            self.put(p, encode({'head': 'a'*40, 'tree': 'b'*40, 'implementation_settings': {'artifacts_sha256_json': encode({self.binary.name: hashlib.sha256(self.binary.read_bytes()).hexdigest()}).decode()}}))
        self.manifest = {'format':'s3-dev-local-runtime/1', 'scope':'REVIEWED_RUNTIME', 'candidate_manifest_sha256':CANDIDATE, 'files_sha256':self.hashes, 'components':components, 'contract_sha256':aggregate(self.hashes)}
        self.seal()
    def put(self, name, raw):
        p = self.bundle / 'files' / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(raw)
        self.hashes[name] = hashlib.sha256(raw).hexdigest()
    def seal(self):
        raw = encode(self.manifest)
        (self.bundle / 'runtime-manifest.json').write_bytes(raw)
        self.pin = hashlib.sha256(raw).hexdigest()
    def check(self, **kw):
        args = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin, profile='s3-dev-local/1', acknowledge=True)
        args.update(kw)
        return verify(**args)
    def test_bytes_are_not_approval(self):
        r = self.check()
        self.assertTrue(r['byte_match'])
        self.assertFalse(r['approval_verified'])
        self.assertFalse(r['services_started'])
        self.assertEqual(r['DEV'], 'NOT_RUN')
    def test_optins_and_pin(self):
        for kw in ({'profile':'standard'}, {'acknowledge':False}, {'pin':'a'*64}, {'pin':'bad'}):
            with self.assertRaises(ValueError): self.check(**kw)
    def test_changed_binary(self):
        self.binary.write_bytes(b'CHANGED')
        with self.assertRaises(ValueError): self.check()
    def test_changed_bundle(self):
        (self.bundle / 'files' / next(iter(self.inherited))).write_bytes(b'CHANGED')
        with self.assertRaises(ValueError): self.check()
    def test_manifest_self_hash_and_duplicates(self):
        p = self.bundle / 'runtime-manifest.json'
        p.write_bytes(p.read_bytes() + b' ')
        with self.assertRaises(ValueError): self.check()
        p.write_bytes(b'{"x":1,"x":1}')
        self.pin = hashlib.sha256(p.read_bytes()).hexdigest()
        with self.assertRaises(ValueError): self.check()
    def test_component_and_count(self):
        self.manifest['components']['sre'] = 'elsewhere'
        self.seal()
        with self.assertRaises(ValueError): self.check()
    def test_symlink_and_hardlink(self):
        target = self.artifacts / 'target'
        self.binary.rename(target)
        self.binary.symlink_to(target)
        with self.assertRaises(OSError): self.check()
        self.binary.unlink()
        os.link(target, self.binary)
        with self.assertRaises(ValueError): self.check()
    def test_directory_symlink_and_traversal(self):
        d = self.artifacts / 'link'
        d.symlink_to(self.bundle, target_is_directory=True)
        for path in ('../bundle/runtime-manifest.json', 'link/runtime-manifest.json'):
            with self.assertRaises((ValueError, OSError)): file_digest(self.artifacts, path)
    def test_fifo_and_bounded(self):
        self.binary.unlink()
        os.mkfifo(self.binary)
        with self.assertRaises(ValueError): file_digest(self.artifacts, self.binary.name)
        with self.assertRaises(ValueError): bounded(self.bundle, 'runtime-manifest.json', 4)
    def test_inherited_substitution_even_with_resealed_pin(self):
        path = next(iter(self.inherited))
        del self.hashes[path]
        self.put('test-only/substitute', b'NOT_APPROVED')
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        with self.assertRaises(ValueError): self.check()
    def test_root_alias(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.bundle, target_is_directory=True)
        with self.assertRaises(ValueError): self.check(bundle=alias)

if __name__ == '__main__': unittest.main()
