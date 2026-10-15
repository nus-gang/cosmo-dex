import base64
import hashlib
import unittest
from unittest.mock import patch
import bootstrap_stage as target
import bootstrap_check
import offline_check
from manifest import decode, encode, aggregate
import test_offline_check as fixtures

class BootstrapStageTest(fixtures.PreflightFixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        with patch.object(offline_check, 'VALIDATOR', bootstrap_check.CHECKER):
            fixtures.OfflineCheckTest.prepare(self)
        self.creator = self.artifacts/target.CREATOR
        self.creator.write_bytes(b'TEST_ONLY_NOT_EXECUTABLE')
        path = target.DESCRIPTOR
        descriptor = decode((self.bundle/'files'/path).read_bytes())
        inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
        inventory[target.CREATOR] = hashlib.sha256(self.creator.read_bytes()).hexdigest()
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode(inventory).decode()
        self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        capture = decode(self.input_file.read_bytes())
        capture['runtime_manifest'] = base64.b64encode((self.bundle/'runtime-manifest.json').read_bytes()).decode()
        capture['files'][path] = base64.b64encode((self.bundle/'files'/path).read_bytes()).decode()
        self.input_file.write_bytes(encode(capture))

    def stage(self, **overrides):
        args = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin,
            profile='s3-dev-local/1', acknowledge=True, decision_id='fixture', revisions={},
            inputs=self.root, input_name='input.json', effective_profile=self.root/'profile.json',
            rpc=b'{"fixture":"NOT_CHAIN_PROOF"}', scratch=self.scratch, timeout=2)
        args.update(overrides)
        return target.stage(**args)

    def test_exact_capture_checker_and_private_copies_cleanup(self):
        raw = self.input_file.read_bytes()
        original = target._validate_snapshot
        def validate(captured, *args):
            self.assertEqual(captured, raw)
            self.creator.write_bytes(b'original replaced')
            self.input_file.write_bytes(b'original replaced')
            return original(captured, *args)
        with patch.object(target.approval_gate, 'inspect', return_value={'fixture':1}) as audit, \
             patch.object(target, '_validate_snapshot', side_effect=validate):
            with self.stage() as staged:
                self.assertEqual(staged.capture, raw)
                self.assertEqual(staged.executable.read_bytes(), b'TEST_ONLY_NOT_EXECUTABLE')
                self.assertEqual(staged.rpc_file.read_bytes(), b'{"fixture":"NOT_CHAIN_PROOF"}')
                self.assertEqual(staged.executable.parent.stat().st_mode & 0o777, 0o700)
                self.assertEqual(staged.executable.stat().st_mode & 0o777, 0o500)
                self.assertEqual(staged.rpc_file.stat().st_mode & 0o777, 0o600)
                self.assertEqual(audit.call_count, 2)
        self.assertFalse(staged.executable.exists())
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_denial_and_invalid_input_no_child(self):
        with patch.object(target.approval_gate, 'inspect', return_value={}), \
             patch.object(target, '_validate_snapshot') as child:
            for args in [dict(rpc=b''), dict(rpc=bytearray(b'x')), dict(rpc=b'x'*(target.MAX_RPC+1)),
                         dict(effective_profile='relative'), dict(acknowledge=False)]:
                with self.assertRaises(ValueError), self.stage(**args): self.fail('yielded')
            with patch.object(target.approval_gate, 'inspect', side_effect=ValueError('DENIED')):
                with self.assertRaisesRegex(ValueError,'DENIED'), self.stage(): self.fail('yielded')
            with patch.object(target, 'CREATOR', 'bin/missing'):
                with self.assertRaisesRegex(ValueError,'CREATOR_NOT_IN'), self.stage(): self.fail('yielded')
            self.creator.write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'ARTIFACT_MISMATCH'), self.stage(): self.fail('yielded')
            child.assert_not_called()
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_revoke_semantic_failure_interrupt_and_write_failure_cleanup(self):
        for error in [ValueError('SEMANTIC'), KeyboardInterrupt()]:
            with patch.object(target.approval_gate, 'inspect', return_value={}), \
                 patch.object(target, '_validate_snapshot', side_effect=error):
                with self.assertRaises(type(error)), self.stage(): self.fail('yielded')
            self.assertEqual(list(self.scratch.iterdir()), [])
        with patch.object(target.approval_gate, 'inspect', side_effect=[{}, {'revoked':True}]):
            with self.assertRaisesRegex(ValueError,'APPROVAL_CHANGED'), self.stage(): self.fail('yielded')
        with patch.object(target.approval_gate, 'inspect', return_value={}), \
             patch.object(target.os, 'fsync', side_effect=OSError('fsync')):
            with self.assertRaises(OSError), self.stage(): self.fail('yielded')
        with patch.object(target.approval_gate, 'inspect', return_value={}):
            with self.assertRaises(KeyboardInterrupt), self.stage(): raise KeyboardInterrupt()
        self.assertEqual(list(self.scratch.iterdir()), [])
        self.assertTrue(self.creator.exists())
        self.assertTrue(self.input_file.exists())

    def test_private_byte_mutation_during_check_rejected(self):
        for name in ['s3-local-bootstrap-create','rpc.json']:
            def mutate(*args):
                path = next(self.scratch.glob('bootstrap-stage-*')).joinpath(name)
                path.chmod(0o600)
                path.write_bytes(b'changed')
            with patch.object(target.approval_gate, 'inspect', return_value={}), \
                 patch.object(target, '_validate_snapshot', side_effect=mutate):
                with self.assertRaisesRegex(ValueError,'BOOTSTRAP_STAGED_BYTES_CHANGED'), self.stage(): self.fail('yielded')
            self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
