import contextlib
import copy
import json
import subprocess
import sys
import unittest
from unittest.mock import patch
import storage_crash_cli as cli
import test_storage_crash_cli as fixtures


class RunCliTest(unittest.TestCase):
    def args(self):
        return ['run-reviewed', *fixtures.CrashCliTest.args(self)[1:]]

    invoke = fixtures.CrashCliTest.invoke

    def report(self, code=86, succeeded=False):
        result = None if code == 86 else dict(schema='s3-local-crash-seal-result/1',
            command_succeeded=succeeded, crash_reached=False, crash_verified=False,
            durable_ack=False, DEV='NOT_RUN')
        return dict(child_result=result, child_exit=code, crash_started=True,
            outcome='UNKNOWN' if code == 86 else 'RECORDED_NOT_REACHED',
            crash_verified=False, approval_verified=False, reusable_permit=False,
            replay_verified=False)

    def test_exact_options_unknown_and_not_reached_cleanup_before_output(self):
        for report in (self.report(), self.report(0), self.report(0, True)):
            events = []
            @contextlib.contextmanager
            def stage(*args):
                self.assertNotIn('--enable-storage-crash', args[9])
                self.assertNotIn('--ceo-revision', args[9])
                events.append('stage')
                try: yield 'staged'
                finally: events.append('clean')
            def run(staged, args, audit, **kw):
                self.assertEqual(staged, 'staged')
                self.assertEqual(kw['point'], 'before_wal')
                self.assertEqual(kw['occurrence'], 1)
                self.assertEqual(kw['purpose'], 'NORMAL')
                self.assertTrue(kw['enable'])
                self.assertFalse(kw['stop']())
                audit(); events.append('run'); return report
            with patch.object(cli,'stage',side_effect=stage), patch.object(cli,'run',side_effect=run), \
                 patch.object(cli.approval_gate,'inspect',return_value={}) as audit, \
                 patch.object(cli,'ready') as ready:
                code,out,err = self.invoke(self.args())
            self.assertEqual((code,err),(0,''))
            self.assertEqual(json.loads(out), report)
            self.assertEqual(events,['stage','run','clean'])
            audit.assert_called_once(); ready.assert_not_called()

    def test_malformed_or_overclaiming_reports_rejected(self):
        good = self.report()
        cases = [dict(good, child_exit=True), dict(good, outcome='PASS'),
            dict(good, crash_verified=True), dict(good, replay_verified=0),
            dict(good, child_result={}), dict(good, extra=False), {}, None]
        normal = self.report(0)
        cases.extend([dict(normal, child_exit=False), dict(normal, outcome='UNKNOWN')])
        for key,value in [('command_succeeded',1), ('crash_reached',True),
                          ('durable_ack',True), ('schema','s3-local-fault-seal-result/1')]:
            changed=copy.deepcopy(normal); changed['child_result'][key]=value; cases.append(changed)
        for report in cases:
            with patch.object(cli,'stage',return_value=contextlib.nullcontext('s')), \
                 patch.object(cli,'run',return_value=report):
                self.assertEqual(self.invoke(self.args()),
                    (2,'','LOCAL_STORAGE_CRASH_RUN_REJECTED_OUTCOME_UNKNOWN\n'))

    def test_error_interrupt_cleanup_and_bad_options(self):
        for error in (ValueError('SECRET'), OSError('/key'), KeyboardInterrupt()):
            for at_cleanup in (False,True):
                @contextlib.contextmanager
                def stage(*args):
                    yield 's'
                    if at_cleanup: raise error
                with patch.object(cli,'stage',side_effect=stage), \
                     patch.object(cli,'run',return_value=self.report(),
                                  side_effect=None if at_cleanup else error):
                    self.assertEqual(self.invoke(self.args()),
                        (2,'','LOCAL_STORAGE_CRASH_RUN_REJECTED_OUTCOME_UNKNOWN\n'))
        with patch.object(cli,'stage') as stage:
            for extra in (['--fault-errno','EIO'], ['--fault-command','Unknown'], ['--enable-storage-crash']):
                self.assertEqual(self.invoke(self.args()+extra)[0:2],(2,''))
            stage.assert_not_called()

    def test_open_stdin_immediate_denial(self):
        p=subprocess.Popen([sys.executable,'-B',cli.__file__,'run-reviewed'],
                           stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(p.wait(timeout=3),2)
            self.assertEqual(p.stdout.read(),b'')
            self.assertEqual(p.stderr.read(),b'LOCAL_STORAGE_CRASH_RUN_REJECTED_OUTCOME_UNKNOWN\n')
        finally:
            if p.poll() is None: p.kill()
            p.wait()
            for stream in (p.stdin,p.stdout,p.stderr): stream.close()
