import contextlib
import json
import subprocess
import sys
import unittest
from unittest.mock import patch

import before_send_cli as cli
import test_before_send_cli as fixtures


class BeforeSendRunCliTest(unittest.TestCase):
    invoke = fixtures.BeforeSendCliTest.invoke

    def args(self):
        return ['run-reviewed', *fixtures.BeforeSendCliTest.args(self)[1:]]

    @staticmethod
    def report():
        return {
            'child_result': None, 'child_exit': 86, 'fault_started': True,
            'outcome': 'UNKNOWN', 'transport_called_verified': False,
            'crash_verified': False, 'approval_verified': False,
            'reusable_permit': False, 'replay_verified': False,
            'F05_verified': False,
        }

    def test_exact_arguments_scope_and_unknown_report(self):
        events = []

        @contextlib.contextmanager
        def stage(*args):
            self.assertNotIn('--tx-hash', args[9])
            events.append('stage')
            try:
                yield 'staged'
            finally:
                events.append('clean')

        def run(staged, args, audit, **kwargs):
            self.assertEqual(staged, 'staged')
            self.assertEqual((kwargs['tx_hash'], kwargs['evidence_root'], kwargs['enable']),
                             ('a' * 64, '/private/fault-evidence', True))
            self.assertFalse(kwargs['stop']())
            audit()
            events.append('run')
            return self.report()

        with patch.object(cli, 'stage', side_effect=stage), patch.object(cli, 'run', side_effect=run), \
             patch.object(cli.approval_gate, 'inspect', return_value={}) as audit:
            code, out, err = self.invoke(self.args())
        self.assertEqual((code, err), (0, ''))
        self.assertEqual(json.loads(out), self.report())
        self.assertEqual(events, ['stage', 'run', 'clean'])
        audit.assert_called_once()

    def test_malformed_or_overclaimed_results_rejected(self):
        good = self.report()
        cases = [None, dict(good, child_exit=True), dict(good, F05_verified=True),
                 dict(good, transport_called_verified=True), dict(good, outcome='PASSED'),
                 dict(good, extra=False)]
        for report in cases:
            with patch.object(cli, 'stage', return_value=contextlib.nullcontext('staged')), \
                 patch.object(cli, 'run', return_value=report):
                self.assertEqual(self.invoke(self.args()),
                                 (2, '', 'LOCAL_F05_RUN_REJECTED_OUTCOME_UNKNOWN\n'))

    def test_errors_interrupt_cleanup_failure(self):
        for error in (ValueError('SECRET'), OSError('/private/key'), KeyboardInterrupt()):
            for during_cleanup in (False, True):
                @contextlib.contextmanager
                def stage(*args):
                    yield 'staged'
                    if during_cleanup:
                        raise error

                with patch.object(cli, 'stage', side_effect=stage), patch.object(
                        cli, 'run', return_value=self.report(),
                        side_effect=None if during_cleanup else error):
                    self.assertEqual(self.invoke(self.args()),
                                     (2, '', 'LOCAL_F05_RUN_REJECTED_OUTCOME_UNKNOWN\n'))

    def test_invalid_arguments_and_open_stdin(self):
        with patch.object(cli, 'stage') as stage:
            self.assertEqual(self.invoke(self.args() + ['--fault-errno', 'EIO']),
                             (2, '', 'LOCAL_F05_RUN_REJECTED_OUTCOME_UNKNOWN\n'))
            stage.assert_not_called()
        process = subprocess.Popen([sys.executable, '-B', cli.__file__, 'run-reviewed'],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE)
        try:
            self.assertEqual(process.wait(timeout=3), 2)
            self.assertEqual(process.stdout.read(), b'')
            self.assertEqual(process.stderr.read(), b'LOCAL_F05_RUN_REJECTED_OUTCOME_UNKNOWN\n')
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()
            for stream in (process.stdin, process.stdout, process.stderr):
                stream.close()
