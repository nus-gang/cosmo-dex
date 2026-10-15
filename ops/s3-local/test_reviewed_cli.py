import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch
import reviewed_cli
import test_offline_cli as offline_tests
import test_reviewed_check as reviewed_tests
from test_native_review import uid
from test_review_documents import REVISIONS
import approval_gate


class ReviewedCliTest(unittest.TestCase):
    def args(self):
        return offline_tests.OfflineCliTest.args(self) + [
            '--native-decision-id', uid(5), '--ceo-revision', REVISIONS['ceo'],
            '--cto-revision', REVISIONS['cto']]

    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = reviewed_cli.main(args)
        return code, out.getvalue(), err.getvalue()

    def test_exact_references_and_private_argument_boundary(self):
        with patch.object(reviewed_cli, 'check', return_value={'approval_verified':False}) as check:
            code, out, err = self.invoke(self.args())
        self.assertEqual((code, err), (0, ''))
        self.assertEqual(json.loads(out), {'approval_verified':False})
        args = check.call_args.args
        self.assertEqual(args[5:9], (uid(5), REVISIONS, Path('/inputs'), 'input.json'))
        self.assertNotIn('--native-decision-id', args[9])
        self.assertNotIn('--ceo-revision', args[9])
        self.assertNotIn('--cto-revision', args[9])

    def test_bad_or_missing_references_rejected_before_io(self):
        good = self.args()
        cases = [[], good[:-2], good+['--serve'], good+['--api-token','SECRET'],
                 good+['--ceo-revision',uid(1)],
                 [x.replace('--native-decision-id','--native-decision') for x in good],
                 [x.replace(uid(5),'not-a-uuid') for x in good],
                 [x for x in good if x != '--acknowledge-unproven-space']]
        with patch.object(reviewed_cli, 'check') as check:
            for args in cases:
                self.assertEqual(self.invoke(args), (2,'','LOCAL_REVIEWED_CHECK_REJECTED\n'))
            check.assert_not_called()

    def test_api_or_validator_error_never_leaks_success_or_diagnostics(self):
        for error in [ValueError('SECRET'), OSError('/private-key'), KeyboardInterrupt()]:
            with patch.object(reviewed_cli, 'check', side_effect=error):
                self.assertEqual(self.invoke(self.args()), (2,'','LOCAL_REVIEWED_CHECK_REJECTED\n'))

    def test_subprocess_rejects_without_waiting_for_stdin(self):
        proc = subprocess.Popen([sys.executable,'-B',reviewed_cli.__file__,'--serve'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(proc.wait(timeout=3),2)
            self.assertEqual(proc.stdout.read(),b'')
            self.assertEqual(proc.stderr.read(),b'LOCAL_REVIEWED_CHECK_REJECTED\n')
        finally:
            if proc.poll() is None: proc.kill()
            proc.wait()
            for stream in [proc.stdin,proc.stdout,proc.stderr]: stream.close()


class ReviewedCliIntegration(reviewed_tests.ReviewedCheckTest):
    # Explicit invocation below runs only the new test; inherited tests are not
    # counted as new coverage.
    def test_cli_through_audit_capture_child_and_final_audit(self):
        args = ReviewedCliTest.args(self)
        mapping = {'a'*64:self.pin, '/bundle':str(self.bundle), '/artifacts':str(self.artifacts),
                   '/inputs/input.json':str(self.root/'input.json'), '/scratch':str(self.scratch)}
        args = [mapping.get(x,x) for x in args]
        with patch.object(approval_gate.Reader,'from_environment',return_value=self.reader):
            code,out,err = ReviewedCliTest.invoke(self,args)
        self.assertEqual((code,err),(0,''))
        report = json.loads(out)
        self.assertIs(report['approval_verified'],False)
        self.assertIs(report['reusable_permit'],False)
        self.assertIs(report['services_started'],False)
        self.assertEqual(len(self.calls),24)
        self.assertEqual(list(self.scratch.iterdir()),[])
        self.issue['status']='in_progress'
        with patch.object(approval_gate.Reader,'from_environment',return_value=self.reader):
            self.assertEqual(ReviewedCliTest.invoke(self,args),
                             (2,'','LOCAL_REVIEWED_CHECK_REJECTED\n'))
