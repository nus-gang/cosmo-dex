from pathlib import Path
import unittest
from unittest.mock import patch
import bootstrap_check
import offline_check
import test_offline_check as fixtures


class BootstrapCheckTest(fixtures.PreflightFixture, unittest.TestCase):
    def prepare(self, tail=None):
        with patch.object(offline_check, 'VALIDATOR', bootstrap_check.CHECKER):
            fixtures.OfflineCheckTest.prepare(self, tail)

    def check(self, **kwargs):
        values = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin,
            profile='s3-dev-local/1', acknowledge=True, decision_id='fixture',
            revisions={'ceo': 'fixture', 'cto': 'fixture'}, inputs=self.root,
            input_name='input.json', effective_profile=self.root/'profile.json',
            scratch=self.scratch, timeout=2)
        values.update(kwargs)
        return bootstrap_check.check(**values)

    def test_exact_checker_argv_capture_private_copy_cleanup(self):
        self.prepare()
        original = offline_check.validate_captured
        def run(executable, arguments, raw, timeout):
            self.assertEqual(arguments, ['--runtime-pin', self.pin,
                '--local-demo-profile', str(self.root/'profile.json'),
                '--acknowledge-unproven-space'])
            self.assertEqual(raw, self.input_file.read_bytes())
            self.assertEqual(Path(executable).stat().st_mode & 0o777, 0o500)
            self.assertEqual(Path(executable).parent.stat().st_mode & 0o777, 0o700)
            self.validator.write_bytes(b'replaced original')
            return original(executable, arguments, raw, timeout)
        with patch.object(bootstrap_check.approval_gate, 'inspect', return_value={'fixture':1}) as audit, \
             patch.object(offline_check, 'validate_captured', side_effect=run):
            report=self.check()
        self.assertEqual(audit.call_count,2)
        self.assertFalse(report['home_created'])
        self.assertFalse(report['reusable_permit'])
        self.assertFalse((self.root/'home').exists())
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_denied_audit_no_capture_or_child_and_no_validator_fallback(self):
        self.prepare()
        with patch.object(bootstrap_check.approval_gate, 'inspect', side_effect=ValueError('DENIED')), \
             patch.object(bootstrap_check, 'verify_input_set') as capture:
            with self.assertRaisesRegex(ValueError,'DENIED'): self.check()
            capture.assert_not_called()
        with patch.object(bootstrap_check.approval_gate, 'inspect', return_value={}), \
             patch.object(bootstrap_check, 'CHECKER', offline_check.VALIDATOR), \
             patch.object(offline_check, 'validate_captured') as child:
            with self.assertRaisesRegex(ValueError,'VALIDATOR_NOT_IN_SRE_DESCRIPTOR'): self.check()
            child.assert_not_called()

    def test_changed_audit_semantic_failure_and_interrupt_cleanup(self):
        self.prepare()
        with patch.object(bootstrap_check.approval_gate, 'inspect', side_effect=[{'v':1},{'v':2}]):
            with self.assertRaisesRegex(ValueError,'APPROVAL_CHANGED'): self.check()
        for error in (ValueError('VALIDATOR_REJECTED'),KeyboardInterrupt()):
            with patch.object(bootstrap_check.approval_gate, 'inspect', return_value={}) as audit, \
                 patch.object(offline_check, 'validate_captured', side_effect=error):
                with self.assertRaises(type(error)): self.check()
                self.assertEqual(audit.call_count,1)
            self.assertEqual(list(self.scratch.iterdir()), [])
        self.assertTrue(self.input_file.exists())
        self.assertTrue(self.validator.exists())

    def test_changed_checker_bytes_and_missing_optin_reject_before_child(self):
        self.prepare()
        original=bootstrap_check.verify_input_set
        def capture(*args):
            result=original(*args)
            self.validator.write_bytes(b'changed')
            return result
        with patch.object(bootstrap_check.approval_gate, 'inspect', return_value={}), \
             patch.object(offline_check, 'validate_captured') as child:
            with self.assertRaises(ValueError): self.check(acknowledge=False)
            with self.assertRaisesRegex(ValueError,'INPUT_PATH'): self.check(effective_profile='relative')
            with patch.object(bootstrap_check, 'verify_input_set', side_effect=capture):
                with self.assertRaisesRegex(ValueError,'VALIDATOR_BYTES_CHANGED'): self.check()
            child.assert_not_called()
        self.assertEqual(list(self.scratch.iterdir()), [])


if __name__ == '__main__': unittest.main()
