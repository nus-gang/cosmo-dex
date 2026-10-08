from contextlib import contextmanager, redirect_stdout, redirect_stderr
import io
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch, Mock

import web_session_cli as target
import test_web_cli as fixtures


class WebSessionCliTest(unittest.TestCase):
    def args(self, fee='0'):
        return ['run-web-reviewed', '--python', sys.executable, '--candidate', '/candidate',
                '--fee-bps', fee, '--workspace-id', '11111111-1111-4111-8111-111111111111',
                '--duration-seconds', '2', '--', *fixtures.WebCliTest.args(self)[1:]]

    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err): rc = target.main(args)
        return rc, out.getvalue(), err.getvalue()

    def test_both_fees_bounded_session_stop_and_fixed_report(self):
        for fee in ('0', '25'):
            events = []
            @contextmanager
            def session(python, candidate, argv, workspace, *, fee_bps, mailbox, stop):
                self.assertEqual((python,candidate,argv,workspace,fee_bps),
                    (sys.executable,'/candidate',self.args(fee)[12:],
                     '11111111-1111-4111-8111-111111111111',int(fee)))
                self.assertFalse(stop()); events.append('start')
                evidence = {}
                try: yield evidence
                finally:
                    events.append('stop')
                    evidence.update(control_plane_stop_verified=True,
                        host_release_observations_complete=True, pid_handoff_collected=True,
                        secret='DO_NOT_OUTPUT')
            with patch.object(target, 'WebMailbox') as prepared, \
                 patch.object(target, 'session', session), \
                 patch.object(target.time, 'monotonic', side_effect=[10,10,12]), \
                 patch.object(target.time, 'sleep') as sleep:
                rc,out,err = self.invoke(self.args(fee))
                prepared.assert_called_once_with(Path('/private/web-pids'))
                prepared.return_value.collect.assert_not_called()
                sleep.assert_called_once_with(0.1)
            self.assertEqual((rc,err,events), (0,'',['start','stop']))
            self.assertNotIn('DO_NOT_OUTPUT',out)
            self.assertIn('"cleanup_complete_verified": false',out)

    def test_invalid_arguments_have_no_effect(self):
        args = self.args()
        cases = [[], ['serve'], [*args, '--unknown'], [*args, '--pid-mailbox', '/duplicate']]
        for index,value in ((2,'python'),(4,'/a/../b'),(6,'1'),(8,'invalid'),
                            (10,'0'),(10,'241'),(10,'02'),(11,'--extra')):
            a=args.copy(); a[index]=value; cases.append(a)
        for a in cases:
            with patch.object(target, 'WebMailbox') as prepared, patch.object(target, 'session') as session:
                self.assertEqual(self.invoke(a), (2,'',target.ERROR+'\n'))
                prepared.assert_not_called(); session.assert_not_called()

    def test_errors_interrupt_and_bad_completion_are_closed(self):
        for failure in (ValueError('SECRET'), KeyboardInterrupt(), None):
            events=[]
            @contextmanager
            def session(*args, **kwargs):
                try:
                    if failure is not None: raise failure
                    yield {}
                finally: events.append('exit')
            with patch.object(target, 'WebMailbox'), patch.object(target, 'session', session), \
                 patch.object(target.time,'monotonic',side_effect=[0,2]):
                self.assertEqual(self.invoke(self.args()),(2,'',target.ERROR+'\n'))
            self.assertEqual(events,['exit'])

    def test_cli_denial_does_not_wait_for_stdin(self):
        p=subprocess.Popen([sys.executable,'-B',str(Path(target.__file__)),'serve'],
                           stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(p.wait(timeout=3),2)
            self.assertEqual(p.stdout.read(),b'')
            self.assertEqual(p.stderr.read(),(target.ERROR+'\n').encode())
        finally:
            if p.poll() is None: p.kill(); p.wait()
            p.stdin.close(); p.stdout.close(); p.stderr.close()


if __name__ == '__main__': unittest.main()
