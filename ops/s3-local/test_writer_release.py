import unittest
from unittest.mock import Mock
from process_check import EXPECTED
import writer_release as wr


class WriterReleaseTest(unittest.TestCase):
    def call(self, validate, times=(10, 20), raw=b'original', arguments=('--home', '/private/home')):
        return wr._check(raw, '/artifacts', arguments, '/scratch', 2,
                         validate, iter(times).__next__)

    def test_exact_capture_arguments_one_call_and_limited_claims(self):
        validate = Mock(return_value=('a'*64, dict(EXPECTED)))
        report = self.call(validate)
        validate.assert_called_once_with(b'original', '/artifacts',
            ('--home', '/private/home'), '/scratch', 2)
        self.assertTrue(report['writer_reopen_verified'])
        for flag in ('continuous_exclusion_verified', 'process_exit_verified',
                     'port_release_verified', 'commit_unchanged_verified',
                     'approval_verified', 'services_started'):
            self.assertIs(report[flag], False)
        self.assertEqual(report['finished_monotonic_ns'], 20)

    def test_busy_corrupt_changed_bytes_io_and_interrupt_never_retry(self):
        for error in (ValueError('BUSY'), ValueError('RECOVERY_REQUIRED'),
                      ValueError('VALIDATOR_BYTES_CHANGED'), OSError('secret'),
                      KeyboardInterrupt(), SystemExit()):
            with self.subTest(error=type(error).__name__):
                validate = Mock(side_effect=error)
                if isinstance(error, (KeyboardInterrupt, SystemExit)):
                    with self.assertRaises(type(error)): self.call(validate)
                else:
                    with self.assertRaisesRegex(ValueError, '^'+wr.ERROR+'$'):
                        self.call(validate)
                validate.assert_called_once()

    def test_untrusted_success_report_digest_and_clock_refuse_completion(self):
        bad_reports = [dict(EXPECTED, semantic_validation=1),
                       dict(EXPECTED, service_started=True), {}, None,
                       dict(EXPECTED, extra=False)]
        for report in bad_reports:
            with self.assertRaisesRegex(ValueError, wr.ERROR):
                self.call(Mock(return_value=('a'*64, report)))
        for digest in ('A'*64, 'a'*63, None):
            with self.assertRaisesRegex(ValueError, wr.ERROR):
                self.call(Mock(return_value=(digest, dict(EXPECTED))))
        for times in ((20, 10), (False, 20), (10, True)):
            with self.assertRaisesRegex(ValueError, wr.ERROR):
                self.call(Mock(return_value=('a'*64, dict(EXPECTED))), times)

    def test_invalid_inputs_rejected_before_validator(self):
        for raw, args in ((b'', []), ('raw', []), (b'raw', '--home'), (b'raw', [1])):
            validate = Mock()
            with self.assertRaisesRegex(ValueError, wr.ERROR):
                self.call(validate, raw=raw, arguments=args)
            validate.assert_not_called()



# Exercise real descriptor hashing, private-copy execution and cleanup with a
# synthetic validator. This is not a real C store or runtime approval test.
import test_offline_check as offline_fixture
from preflight import verify_input_set
from test_preflight import PreflightFixture

class WriterTransportTest(PreflightFixture, unittest.TestCase):
    prepare = offline_fixture.OfflineCheckTest.prepare
    def test_writer_transport(self):
        self.prepare()
        raw, _ = verify_input_set(self.bundle, self.artifacts, self.pin,
            's3-dev-local/1', True, self.root, 'input.json')
        report = wr.check(raw, self.artifacts, [], self.scratch, 2)
        self.assertTrue(report['writer_reopen_verified'])
        self.assertEqual(list(self.scratch.iterdir()), [])
        self.validator.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, wr.ERROR):
            wr.check(raw, self.artifacts, [], self.scratch, 2)
        self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
