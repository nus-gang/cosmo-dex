import contextlib
import io
import subprocess
import sys
import unittest
from unittest.mock import patch
import managed_cli
from test_reviewed_cli import ReviewedCliTest


class ManagedCliTest(unittest.TestCase):
    def setUp(self):
        self.reporter = patch.object(managed_cli, 'reporter').start()
        self.addCleanup(patch.stopall)

    def args(self):
        return ['serve-reviewed', *ReviewedCliTest.args(self), '--approval-socket', '/private/broker/s', '--pid-mailbox', '/private/session/pids']

    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = managed_cli.main(args)
        return code, out.getvalue(), err.getvalue()

    def test_foreground_arguments_and_silent_exit(self):
        with patch.object(managed_cli, 'run', return_value={
                'worker_exit': 0, 'reusable_permit': False}) as run:
            self.assertEqual(self.invoke(self.args()), (0, '', ''))
        args = run.call_args.args
        self.assertNotIn('--native-decision-id', args[9])
        self.assertNotIn('--cto-revision', args[9])
        self.assertEqual(run.call_args.kwargs, {'lifetime': int(self.args()[self.args().index('--lifetime-seconds')+1]), 'on_spawn': self.reporter.return_value})

    def test_denials_before_runtime_io(self):
        good = self.args()
        cases = [[], good[1:], ['serve', *good[1:]], good+['--serve'],
                 good+['--api-token', 'SECRET'], good+['--max-ticks','2'],
                 [x for x in good if x != '--acknowledge-unproven-space']]
        for value in ['0','301','01','+1','1.5','NaN','١']:
            args = good.copy()
            args[args.index('--lifetime-seconds')+1] = value
            cases.append(args)
        with patch.object(managed_cli, 'run') as run:
            for args in cases:
                self.assertEqual(self.invoke(args),(2,'','LOCAL_MANAGED_WORKER_REJECTED\n'))
            run.assert_not_called()

    def test_errors_and_invalid_report_do_not_leak(self):
        for error in [ValueError('SECRET'), OSError('/keys/private'), KeyboardInterrupt()]:
            with patch.object(managed_cli, 'run', side_effect=error):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_MANAGED_WORKER_REJECTED\n'))
        with patch.object(managed_cli, 'run', return_value={'worker_exit': 1}):
            self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_MANAGED_WORKER_REJECTED\n'))

    def test_real_cli_denial_with_open_stdin(self):
        proc = subprocess.Popen([sys.executable,'-B',managed_cli.__file__,'serve-reviewed'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(proc.wait(timeout=3),2)
            self.assertEqual(proc.stdout.read(),b'')
            self.assertEqual(proc.stderr.read(),b'LOCAL_MANAGED_WORKER_REJECTED\n')
        finally:
            if proc.poll() is None: proc.kill()
            proc.wait()
            for stream in (proc.stdin,proc.stdout,proc.stderr): stream.close()
