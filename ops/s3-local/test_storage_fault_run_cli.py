import contextlib
import copy
import json
import subprocess
import sys
import unittest
from unittest.mock import patch
import storage_fault_cli as cli
import test_storage_fault_cli as fixtures


class RunCliTest(unittest.TestCase):
    def args(self):
        return ['run-reviewed', *fixtures.FaultCliTest.args(self)[1:]]

    invoke = fixtures.FaultCliTest.invoke

    def report(self):
        return dict(child_exit=0, fault_started=True, approval_verified=False,
                    reusable_permit=False, replay_verified=False,
                    child_result=dict(schema='s3-local-fault-seal-result/1',
                        durable_ack=False, DEV='NOT_RUN', command_succeeded=False, injected=True))

    def test_apply_explicit_command_and_mismatched_result_rejection(self):
        report=self.report()
        report['child_result']['schema']='s3-local-fault-apply-result/1'
        args=self.args()+['--fault-command','Apply']
        with patch.object(cli,'stage',return_value=contextlib.nullcontext('staged')), \
             patch.object(cli,'run',return_value=report) as run:
            self.assertEqual(self.invoke(args)[0],0)
            self.assertEqual(run.call_args.kwargs['operation'],'Apply')
            run.return_value=self.report()
            self.assertEqual(self.invoke(args)[0],2)
        for extra in (['--fault-command','Auto'],['--fault-command','Apply','--fault-command','Apply']):
            with patch.object(cli,'stage') as stage:
                self.assertEqual(self.invoke(self.args()+extra)[0],2)
                stage.assert_not_called()
        args[args.index('NORMAL')]='RESOLVE_FAILURE'
        with patch.object(cli,'stage') as stage:
            self.assertEqual(self.invoke(args)[0],2)
            stage.assert_not_called()

    def test_exact_wiring_scopes_cleanup_and_independent_outcomes(self):
        for success in (False, True):
            for injected in (False, True):
                events=[]
                report=self.report()
                report['child_result'].update(command_succeeded=success,injected=injected)
                @contextlib.contextmanager
                def stage(*args):
                    self.assertNotIn('--fault-point',args[9])
                    self.assertNotIn('--ceo-revision',args[9])
                    events.append('stage')
                    yield 'staged'
                    events.append('clean')
                def run(staged,args,audit,**kw):
                    self.assertEqual(staged,'staged')
                    self.assertEqual(kw['point'],'before_wal')
                    self.assertEqual(kw['occurrence'],1)
                    self.assertEqual(kw['purpose'],'NORMAL')
                    self.assertEqual(kw['evidence_root'],'/private/fault-evidence')
                    self.assertTrue(kw['enable'])
                    self.assertFalse(kw['stop']())
                    audit(); events.append('run-reaped')
                    return report
                with patch.object(cli,'stage',side_effect=stage), patch.object(cli,'run',side_effect=run), \
                     patch.object(cli,'ready') as ready, patch.object(cli.approval_gate,'inspect',return_value={}) as audit:
                    code,out,err=self.invoke(self.args())
                self.assertEqual((code,err),(0,''))
                self.assertEqual(json.loads(out),report)
                self.assertEqual(events,['stage','run-reaped','clean'])
                ready.assert_not_called(); audit.assert_called_once()

    def test_invalid_reports_errors_and_cleanup_never_emit_success(self):
        reports=[None,{},dict(self.report(),replay_verified=True),dict(self.report(),child_exit=False)]
        for key,value in [('durable_ack',True),('command_succeeded',1),('injected','true'),('DEV','PASS')]:
            report=self.report(); report['child_result'][key]=value; reports.append(report)
        for report in reports:
            with patch.object(cli,'stage',return_value=contextlib.nullcontext('staged')), \
                 patch.object(cli,'run',return_value=report):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_STORAGE_FAULT_RUN_REJECTED_OUTCOME_UNKNOWN\n'))
        for error in (ValueError('SECRET'),OSError('/private/key'),KeyboardInterrupt()):
            for cleanup in (False,True):
                @contextlib.contextmanager
                def stage(*args):
                    yield 'staged'
                    if cleanup: raise error
                with patch.object(cli,'stage',side_effect=stage), \
                     patch.object(cli,'run',return_value=self.report(),side_effect=None if cleanup else error):
                    self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_STORAGE_FAULT_RUN_REJECTED_OUTCOME_UNKNOWN\n'))

    def test_errno_reaches_execution_supervisor(self):
        for value in ('ENOSPC','EDQUOT','EIO'):
            with patch.object(cli,'stage',return_value=contextlib.nullcontext('staged')), \
                 patch.object(cli,'run',return_value=self.report()) as run:
                self.assertEqual(self.invoke(self.args()+['--fault-errno',value])[0],0)
                self.assertEqual(run.call_args.kwargs['errno'],value)

    def test_bad_options_before_stage_and_run(self):
        good=self.args()
        cases=[good+['--start'],good+['--lifetime','61'],good+['--fault-occurrence','2'],
               [x for x in good if x!='--enable-storage-fault'],
               [x for x in good if x!='--acknowledge-unproven-space']]
        with patch.object(cli,'stage') as stage,patch.object(cli,'run') as run:
            for args in cases:
                self.assertEqual(self.invoke(args),(2,'','LOCAL_STORAGE_FAULT_RUN_REJECTED_OUTCOME_UNKNOWN\n'))
            stage.assert_not_called(); run.assert_not_called()

    def test_open_stdin_denial(self):
        child=subprocess.Popen([sys.executable,'-B',cli.__file__,'run-reviewed'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(child.wait(timeout=3),2)
            self.assertEqual(child.stdout.read(),b'')
            self.assertEqual(child.stderr.read(),b'LOCAL_STORAGE_FAULT_RUN_REJECTED_OUTCOME_UNKNOWN\n')
        finally:
            if child.poll() is None: child.kill()
            child.wait()
            for stream in (child.stdin,child.stdout,child.stderr): stream.close()
