import contextlib
import io
import subprocess
import sys
import unittest
from unittest.mock import patch
import web_cli
import test_reviewed_cli as fixtures


class WebCliTest(unittest.TestCase):
    def setUp(self):
        self.reporter = patch.object(web_cli, "reporter", return_value=lambda: None).start()
        self.addCleanup(patch.stopall)

    def args(self):
        return ['serve-web-reviewed', *fixtures.ReviewedCliTest.args(self),
                '--pid-mailbox', '/private/web-pids', '--approval-socket', '/private/broker/s', '--web-origin', 'http://127.0.0.1:5173']

    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = web_cli.main(args)
        return code, out.getvalue(), err.getvalue()

    def test_scopes_arguments_and_silent_success(self):
        events = []
        @contextlib.contextmanager
        def reader(path):
            events.append('reader')
            try: yield
            finally: events.append('reader-close')
        result = dict(listener_closed=True, approval_verified=False, reusable_permit=False,
                      handled_connections=0, capture_sha256='a'*64, validator_sha256='b'*64)
        with patch.object(web_cli, 'private_transport', reader), patch.object(web_cli, 'run', return_value=result) as run:
            self.assertEqual(self.invoke(self.args()), (0, '', ''))
        self.assertEqual(events, ['reader', 'reader-close'])
        self.assertNotIn('--pid-mailbox', run.call_args.args[9])
        self.assertTrue(callable(run.call_args.kwargs['on_ready']))
        self.assertNotIn('--web-origin', run.call_args.args[9])
        self.assertNotIn('--approval-socket', run.call_args.args[9])
        self.assertFalse(run.call_args.kwargs['stop']())
        self.assertEqual(run.call_args.args[-1], int(self.args()[self.args().index('--bind')+1].split(':')[1]))

    def test_invalid_inputs_before_reader_or_run(self):
        good = self.args()
        cases = [[], good[1:], good+['--web-origin', 'http://localhost:5173'],
                 good+['--web-orig', 'x'], good+['--pid-mailbox','/x'],
                 [x for x in good if x != '--acknowledge-unproven-space']]
        for option, values in {'--bind':['0.0.0.0:8787','127.0.0.1:5173'],
                '--lifetime-seconds':['301','01','+1'], '--max-requests':['0','10001'],
                '--approval-socket':['relative','/private/../s'], '--web-origin':['https://localhost:5173']}.items():
            for value in values:
                args = good.copy(); args[args.index(option)+1] = value; cases.append(args)
        with patch.object(web_cli,'private_transport') as reader, patch.object(web_cli,'run') as run:
            for args in cases:
                self.assertEqual(self.invoke(args), (2,'','LOCAL_MANAGED_WEB_REJECTED\n'))
            reader.assert_not_called(); run.assert_not_called()

    def test_failure_restores_scope_without_leaking(self):
        for failure in (ValueError('SECRET'), OSError('SECRET'), KeyboardInterrupt()):
            with patch.object(web_cli,'run',side_effect=failure), patch.object(web_cli,'private_transport',return_value=contextlib.nullcontext()):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_MANAGED_WEB_REJECTED\n'))
        with patch.object(web_cli,'run',return_value={}), patch.object(web_cli,'private_transport',return_value=contextlib.nullcontext()):
            self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_MANAGED_WEB_REJECTED\n'))

    def test_open_stdin_denied_immediately(self):
        proc = subprocess.Popen([sys.executable,'-B',web_cli.__file__,'serve-web-reviewed'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(proc.wait(timeout=3),2)
            self.assertEqual(proc.stdout.read(),b'')
            self.assertEqual(proc.stderr.read(),b'LOCAL_MANAGED_WEB_REJECTED\n')
        finally:
            if proc.poll() is None: proc.kill()
            proc.wait()
            for stream in (proc.stdin,proc.stdout,proc.stderr): stream.close()
