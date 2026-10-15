import contextlib
import copy
import json
import subprocess
import sys
import unittest
from unittest.mock import patch
import correction_cli as cli
import test_correction_cli as fixtures

class RunCliTest(unittest.TestCase):
    def args(self):
        return ['run-reviewed', *fixtures.CorrectionCliTest.args(self)[1:]]
    invoke = fixtures.CorrectionCliTest.invoke

    def report(self, injected=False):
        return dict(child_exit=0,fault_started=True,approval_verified=False,reusable_permit=False,
                    replay_verified=False,F14_verified=False,child_result=dict(
                    schema='s3-local-f14-apply-result/1',command_succeeded=not injected,
                    injected=injected,prepare_visits=int(injected),replay_visits=0,
                    F14_verified=False,durable_ack=False,DEV='NOT_RUN'))

    def test_exact_arguments_scope_and_report(self):
        for injected in (False,True):
            events=[]
            @contextlib.contextmanager
            def stage(*args):
                self.assertNotIn('--fault-occurrence',args[9])
                events.append('stage')
                try: yield 'staged'
                finally: events.append('clean')
            def run(staged,args,audit,**kw):
                self.assertEqual(staged,'staged')
                self.assertEqual((kw['occurrence'],kw['evidence_root'],kw['enable']),
                                 (1,'/private/fault-evidence',True))
                self.assertFalse(kw['stop']())
                audit(); events.append('run'); return self.report(injected)
            with patch.object(cli,'stage',side_effect=stage),patch.object(cli,'run',side_effect=run), \
                 patch.object(cli.approval_gate,'inspect',return_value={}) as audit:
                code,out,err=self.invoke(self.args())
            self.assertEqual((code,err),(0,'')); self.assertEqual(json.loads(out),self.report(injected))
            self.assertEqual(events,['stage','run','clean']); audit.assert_called_once()

    def test_malformed_or_overclaimed_results(self):
        good=self.report(); cases=[None,dict(good,child_exit=False),dict(good,F14_verified=True),
            dict(good,replay_verified=True),dict(good,extra=0)]
        for field,value in [('injected',1),('prepare_visits',True),('schema','wrong'),
                            ('F14_verified',True),('prepare_visits',1),('replay_visits',65537)]:
            r=copy.deepcopy(good); r['child_result'][field]=value; cases.append(r)
        for report in cases:
            with patch.object(cli,'stage',return_value=contextlib.nullcontext('s')), \
                 patch.object(cli,'run',return_value=report):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_F14_RUN_REJECTED_OUTCOME_UNKNOWN\n'))

    def test_errors_interrupt_cleanup_failure(self):
        for error in (ValueError('SECRET'),OSError('/private/key'),KeyboardInterrupt()):
            for during_cleanup in (False,True):
                @contextlib.contextmanager
                def stage(*args):
                    yield 's'
                    if during_cleanup: raise error
                with patch.object(cli,'stage',side_effect=stage),patch.object(cli,'run',
                        return_value=self.report(),side_effect=None if during_cleanup else error):
                    self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_F14_RUN_REJECTED_OUTCOME_UNKNOWN\n'))

    def test_invalid_arguments_open_stdin(self):
        with patch.object(cli,'stage') as stage:
            self.assertEqual(self.invoke(self.args()+['--fault-errno','EIO']),
                             (2,'','LOCAL_F14_RUN_REJECTED_OUTCOME_UNKNOWN\n'))
            stage.assert_not_called()
        p=subprocess.Popen([sys.executable,'-B',cli.__file__,'run-reviewed'],stdin=subprocess.PIPE,
                           stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(p.wait(timeout=3),2)
            self.assertEqual(p.stdout.read(),b'')
            self.assertEqual(p.stderr.read(),b'LOCAL_F14_RUN_REJECTED_OUTCOME_UNKNOWN\n')
        finally:
            if p.poll() is None: p.kill()
            p.wait()
            for s in (p.stdin,p.stdout,p.stderr): s.close()
