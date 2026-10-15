import unittest
from unittest.mock import patch
import approval_gate
import offline_check
import staged_worker
import test_staged_worker as fixtures
from test_native_review import uid
from test_review_documents import REVISIONS

class ValidatedStageTest(fixtures.StagedWorkerTest):
    setUp = fixtures.StagedWorkerTest.setUp
    subject = fixtures.StagedWorkerTest.subject
    reader = fixtures.StagedWorkerTest.reader

    def scope(self):
        return staged_worker.validated_stage(self.bundle, self.artifacts, self.pin,
            's3-dev-local/1', True, uid(5), REVISIONS, self.root, 'input.json',
            [], self.scratch, 2)

    def test_same_capture_after_input_replacement_and_cleanup(self):
        raw = self.input_file.read_bytes()
        original = offline_check.validate_captured
        def run(executable, arguments, captured, timeout):
            self.assertEqual(captured, raw)
            self.input_file.write_bytes(b'REPLACEMENT')
            return original(executable, arguments, captured, timeout)
        with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader), \
             patch.object(offline_check, 'validate_captured', side_effect=run):
            with self.scope() as staged:
                self.assertEqual(staged.capture, raw)
                self.assertTrue(staged.executable.exists())
                self.assertEqual(len(list(self.scratch.iterdir())), 1)
        self.assertFalse(staged.executable.exists())
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_semantic_failure_interrupt_and_private_mutation_never_yield(self):
        def mutate(*args):
            executable = next(self.scratch.glob('staged-worker-*/s3-local-worker'))
            executable.chmod(0o700)
            executable.write_bytes(b'CHANGED')
            return ('unused', {})
        for effect in [ValueError('semantic rejection'), KeyboardInterrupt(), mutate]:
            with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader), \
                 patch.object(offline_check, 'validate_snapshot', side_effect=effect):
                with self.assertRaises((ValueError, KeyboardInterrupt)), self.scope():
                    self.fail('yielded')
            self.assertEqual(list(self.scratch.iterdir()), [])
            self.assertTrue(self.input_file.exists())

    def test_revocation_during_validator_never_yields(self):
        original = offline_check.validate_snapshot
        def revoke(*args):
            result = original(*args)
            self.issue['status'] = 'in_progress'
            return result
        with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader), \
             patch.object(offline_check, 'validate_snapshot', side_effect=revoke):
            with self.assertRaises(ValueError), self.scope(): self.fail('yielded')
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_initial_denial_runs_no_validator(self):
        self.issue['status'] = 'in_progress'
        with patch.object(approval_gate.Reader, 'from_environment', return_value=self.reader), \
             patch.object(offline_check, 'validate_snapshot') as validator:
            with self.assertRaises(ValueError), self.scope(): self.fail('yielded')
            validator.assert_not_called()
        self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
