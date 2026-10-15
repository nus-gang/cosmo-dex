import base64
import hashlib
import unittest
from unittest.mock import patch
import approval_gate
import storage_crash_stage as target
from manifest import aggregate, decode, encode
import test_staged_worker as fixtures
from test_native_review import uid
from test_review_documents import REVISIONS

class CrashStageTest(fixtures.fixtures.PreflightFixture, unittest.TestCase):
    subject = fixtures.StagedWorkerTest.subject
    reader = fixtures.StagedWorkerTest.reader
    def setUp(self):
        fixtures.fixtures.PreflightFixture.setUp(self)
        import json
        from manifest import COMPONENTS, encode, aggregate
        for name in COMPONENTS:
            path = self.manifest['components'][name]
            descriptor = json.loads((self.bundle/'files'/path).read_bytes())
            descriptor['implementation_settings'].update({
                'build_argv_json': encode(['TEST_ONLY', '--locked']).decode(),
                'toolchain': 'TEST_ONLY',
                'approval_sources_json': encode(['TEST_ONLY_NOT_APPROVAL']).decode(),
                'implementation_locks_json': encode({'TEST_ONLY': 'c'*64}).decode()})
            self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        fixtures.fixtures.offline_fixtures.OfflineCheckTest.prepare(self)
        self.issue = fixtures.fixtures.approved()
        self.calls = []
        from test_review_documents import documents
        self.rows = documents()
        bound = approval_gate.bound_subject(self.subject(), uid(5))
        for row, role in zip(self.rows, fixtures.review_documents.AUTHORS):
            row['body'] = fixtures.review_documents.approval_body(bound, role)

        self.worker = self.artifacts / fixtures.staged_worker.WORKER
        self.worker.write_bytes(b'TEST_ONLY_NEVER_EXECUTED')
        path = fixtures.staged_worker.DESCRIPTOR
        descriptor = decode((self.bundle/'files'/path).read_bytes())
        inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
        inventory[fixtures.staged_worker.WORKER] = hashlib.sha256(self.worker.read_bytes()).hexdigest()
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode(inventory).decode()
        self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        capture = decode(self.input_file.read_bytes())
        capture['runtime_manifest'] = base64.b64encode((self.bundle/'runtime-manifest.json').read_bytes()).decode()
        capture['files'][path] = base64.b64encode((self.bundle/'files'/path).read_bytes()).decode()
        self.input_file.write_bytes(encode(capture))
        bound = approval_gate.bound_subject(self.subject(), uid(5))
        for row, role in zip(self.rows, fixtures.review_documents.AUTHORS):
            row['body'] = fixtures.review_documents.approval_body(bound, role)
        self.binary = self.artifacts / target.CRASH
        self.binary.write_bytes(b'CRASH_BUILD_TEST_ONLY_NOT_EXECUTED')
        path = target.DESCRIPTOR
        descriptor = decode((self.bundle/'files'/path).read_bytes())
        inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
        inventory[target.CRASH] = hashlib.sha256(self.binary.read_bytes()).hexdigest()
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode(inventory).decode()
        self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        capture = decode(self.input_file.read_bytes())
        capture['runtime_manifest'] = base64.b64encode((self.bundle/'runtime-manifest.json').read_bytes()).decode()
        capture['files'][path] = base64.b64encode((self.bundle/'files'/path).read_bytes()).decode()
        self.input_file.write_bytes(encode(capture))
    def scope(self, **kw):
        args = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin,
            profile='s3-dev-local/1', acknowledge=True, decision_id=uid(5),
            revisions=REVISIONS, inputs=self.root, input_name='input.json',
            arguments=['TEST_VALIDATOR_ARGS'], scratch=self.scratch, timeout=2)
        args.update(kw)
        return target.stage(**args)
    def test_same_capture_private_bytes_source_swap_and_cleanup(self):
        raw = self.input_file.read_bytes(); binary = self.binary.read_bytes()
        def validate(captured, artifacts, arguments, scratch, timeout):
            self.assertEqual((captured, arguments, timeout), (raw, ['TEST_VALIDATOR_ARGS'], 2))
            self.binary.write_bytes(b'REPLACED'); self.input_file.write_bytes(b'REPLACED')
        with patch.object(approval_gate, 'inspect', return_value={'test': True}) as audit, \
             patch.object(target, 'validate_snapshot', side_effect=validate):
            with self.scope() as staged:
                self.assertEqual(staged.capture, raw)
                self.assertEqual(staged.executable.read_bytes(), binary)
                self.assertEqual(staged.executable.stat().st_mode & 0o777, 0o500)
                self.assertEqual(staged.executable.parent.stat().st_mode & 0o777, 0o700)
                self.assertEqual(audit.call_count, 2)
            self.assertFalse(staged.executable.exists())
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_gate_optin_descriptor_and_binary_rejection(self):
        with patch.object(approval_gate, 'inspect', side_effect=ValueError('denied')), \
             patch.object(target, 'validate_snapshot') as validator:
            with self.assertRaises(ValueError), self.scope(): self.fail('yield')
            validator.assert_not_called()
        with patch.object(approval_gate, 'inspect', return_value={}), \
             patch.object(target, 'validate_snapshot') as validator:
            for kw in [dict(acknowledge=False), dict(profile='standard')]:
                with self.assertRaises(ValueError), self.scope(**kw): self.fail('yield')
            captured = decode(self.input_file.read_bytes())
            descriptor = decode(base64.b64decode(captured['files'][target.DESCRIPTOR]))
            inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
            del inventory[target.CRASH]
            descriptor['implementation_settings']['artifacts_sha256_json'] = encode(inventory).decode()
            captured['files'][target.DESCRIPTOR] = base64.b64encode(encode(descriptor)).decode()
            with patch.object(target, 'verify_input_set', return_value=(encode(captured), {})):
                with self.assertRaisesRegex(ValueError, 'CRASH_NOT_IN_SRE_DESCRIPTOR'), self.scope():
                    self.fail('ordinary worker fallback')
            self.binary.write_bytes(b'CHANGED')
            with self.assertRaises(ValueError), self.scope(): self.fail('yield')
            validator.assert_not_called()
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_semantic_error_interrupt_mutation_and_revocation_cleanup(self):
        def mutate(*args):
            p = next(self.scratch.glob('storage-crash-stage-*/s3-local-storage-crash'))
            p.chmod(0o700); p.write_bytes(b'MUTATED'); p.chmod(0o500)
        for effect in [ValueError('semantic'), KeyboardInterrupt(), mutate]:
            with patch.object(approval_gate, 'inspect', return_value={}), \
                 patch.object(target, 'validate_snapshot', side_effect=effect):
                with self.assertRaises((ValueError, KeyboardInterrupt)), self.scope(): self.fail('yield')
            self.assertEqual(list(self.scratch.iterdir()), [])
        with patch.object(approval_gate, 'inspect', side_effect=[{}, {'revoked':True}]), \
             patch.object(target, 'validate_snapshot'):
            with self.assertRaisesRegex(ValueError, 'APPROVAL_CHANGED'), self.scope(): self.fail('yield')
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_fsync_failure_and_consumer_interrupt_preserve_sources(self):
        with patch.object(approval_gate, 'inspect', return_value={}), \
             patch.object(target.os, 'fsync', side_effect=OSError('fsync')):
            with self.assertRaises(OSError), self.scope(): self.fail('yield')
        self.assertEqual(list(self.scratch.iterdir()), [])
        with patch.object(approval_gate, 'inspect', return_value={}), \
             patch.object(target, 'validate_snapshot'):
            with self.assertRaises(KeyboardInterrupt), self.scope(): raise KeyboardInterrupt()
        self.assertEqual(list(self.scratch.iterdir()), [])
        self.assertTrue(self.binary.exists()); self.assertTrue(self.input_file.exists())

    def test_io_binary_is_not_a_crash_fallback_and_links_rejected(self):
        import os
        import storage_fault_stage
        captured = decode(self.input_file.read_bytes())
        descriptor = decode(base64.b64decode(captured['files'][target.DESCRIPTOR]))
        inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
        digest = inventory.pop(target.CRASH)
        inventory[storage_fault_stage.FAULT] = digest
        io_binary = self.artifacts / storage_fault_stage.FAULT
        io_binary.write_bytes(self.binary.read_bytes())
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode(inventory).decode()
        captured['files'][target.DESCRIPTOR] = base64.b64encode(encode(descriptor)).decode()
        with patch.object(approval_gate, 'inspect', return_value={}), \
             patch.object(target, 'validate_snapshot') as validator:
            with patch.object(target, 'verify_input_set', return_value=(encode(captured), {})):
                with self.assertRaisesRegex(ValueError, 'CRASH_NOT_IN_SRE_DESCRIPTOR'), self.scope():
                    self.fail('IO fallback')
            self.binary.unlink()
            self.binary.symlink_to(io_binary)
            with self.assertRaises((ValueError, OSError)), self.scope(): self.fail('symlink')
            self.binary.unlink()
            os.link(io_binary, self.binary)
            with self.assertRaises((ValueError, OSError)), self.scope(): self.fail('hardlink')
            validator.assert_not_called()
        self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
