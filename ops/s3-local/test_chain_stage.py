import base64
import hashlib
import unittest
from unittest.mock import patch
import chain_stage as target
from manifest import decode, encode, aggregate
import test_offline_check as fixtures

class ChainStageTest(fixtures.PreflightFixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        fixtures.OfflineCheckTest.prepare(self)
        self.chain = self.artifacts/target.CHAIN
        self.chain.write_bytes(b'TEST_ONLY_CHAIN_NOT_EXECUTABLE')
        self.profile_file = self.root/'profile.json'
        self.profile_file.write_bytes(b'TEST_ONLY_PROFILE')
        descriptor = decode((self.bundle/'files'/target.DESCRIPTOR).read_bytes())
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode({
            target.CHAIN:hashlib.sha256(self.chain.read_bytes()).hexdigest()}).decode()
        self.put(target.DESCRIPTOR, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        value = decode(self.input_file.read_bytes())
        value['runtime_manifest'] = base64.b64encode((self.bundle/'runtime-manifest.json').read_bytes()).decode()
        value['files'][target.DESCRIPTOR] = base64.b64encode(encode(descriptor)).decode()
        self.input_file.write_bytes(encode(value))

    def stage(self, **overrides):
        args = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin,
            profile='s3-dev-local/1', acknowledge=True, decision_id='fixture', revisions={},
            inputs=self.root, input_name='input.json', effective_profile=self.profile_file,
            scratch=self.scratch)
        args.update(overrides)
        return target.stage(**args)

    def test_exact_private_bytes_original_replacement_cleanup(self):
        original = self.input_file.read_bytes()
        with patch.object(target.approval_gate, 'inspect', return_value={}) as audit:
            with self.stage() as staged:
                self.chain.write_bytes(b'replaced')
                self.input_file.write_bytes(b'replaced')
                self.profile_file.write_bytes(b'replaced')
                staged.verify()
                self.assertEqual(staged.input_set.read_bytes(), original)
                self.assertEqual(staged.effective_profile.read_bytes(), b'TEST_ONLY_PROFILE')
                self.assertEqual(staged.executable.read_bytes(), b'TEST_ONLY_CHAIN_NOT_EXECUTABLE')
                self.assertEqual(staged.executable.parent.stat().st_mode & 0o777, 0o700)
                for path, mode in [(staged.executable,0o500),(staged.input_set,0o600),(staged.effective_profile,0o600)]:
                    self.assertEqual(path.stat().st_mode & 0o777, mode)
                self.assertEqual(audit.call_count,2)
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_denial_missing_descriptor_and_mutation(self):
        with patch.object(target.approval_gate,'inspect',return_value={}):
            with self.assertRaises(ValueError), self.stage(acknowledge=False): self.fail('yielded')
            with patch.object(target,'CHAIN','bin/absent'):
                with self.assertRaisesRegex(ValueError,'CHAIN_NOT_IN'), self.stage(): self.fail('yielded')
            self.chain.write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'ARTIFACT_MISMATCH'), self.stage(): self.fail('yielded')
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_revoke_interrupt_fsync_cleanup(self):
        with patch.object(target.approval_gate,'inspect',side_effect=[{}, {'revoked':True}]):
            with self.assertRaisesRegex(ValueError,'APPROVAL_CHANGED'), self.stage(): self.fail('yielded')
        with patch.object(target.approval_gate,'inspect',return_value={}):
            with self.assertRaises(KeyboardInterrupt), self.stage(): raise KeyboardInterrupt()
            with patch('bootstrap_stage.os.fsync',side_effect=OSError('fsync')):
                with self.assertRaises(OSError), self.stage(): self.fail('yielded')
        self.assertEqual(list(self.scratch.iterdir()),[])
        self.assertTrue(self.input_file.exists())

    def test_staged_mutation_and_symlink_rejected(self):
        with patch.object(target.approval_gate,'inspect',return_value={}):
            for field in ['executable','input_set','effective_profile']:
                with self.stage() as staged:
                    path = getattr(staged, field)
                    path.chmod(0o600)
                    path.write_bytes(b'changed')
                    with self.assertRaisesRegex(ValueError,'CHAIN_STAGED_BYTES_CHANGED'): staged.verify()
                with self.stage() as staged:
                    path = getattr(staged, field)
                    path.unlink()
                    path.symlink_to(self.input_file)
                    with self.assertRaises(OSError): staged.verify()
        self.assertEqual(list(self.scratch.iterdir()),[])

if __name__ == '__main__': unittest.main()
