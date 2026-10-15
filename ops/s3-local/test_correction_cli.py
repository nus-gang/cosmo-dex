import contextlib
import io
import json
import subprocess
import sys
import unittest
from unittest.mock import patch
import correction_cli as cli
import test_reviewed_cli as fixtures


class CorrectionCliTest(unittest.TestCase):
    def args(self):
        return ['check-ready-reviewed', *fixtures.ReviewedCliTest.args(self),
                '--enable-f14-prepare',
                '--fault-occurrence', '1',
                '--fault-evidence-root', '/private/fault-evidence']

    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = cli.main(args)
        return code, out.getvalue(), err.getvalue()

    def test_scopes_exact_arguments_audit_and_cleanup_before_output(self):
        events = []
        report = dict(ready=True, fault_started=False, F14_verified=False, service_started=False,
                      approval_verified=False, reusable_permit=False)
        @contextlib.contextmanager
        def stage(*args):
            self.assertNotIn('--fault-point', args[9])
            self.assertNotIn('--ceo-revision', args[9])
            events.append('stage')
            try: yield 'staged'
            finally: events.append('clean')
        @contextlib.contextmanager
        def ready(staged, args, audit, **kw):
            self.assertEqual(staged, 'staged')
            self.assertEqual(kw['occurrence'], 1)
            self.assertEqual(kw['evidence_root'], '/private/fault-evidence')
            audit()
            events.append('ready')
            try: yield report
            finally: events.append('reap')
        with patch.object(cli,'stage',side_effect=stage), patch.object(cli,'ready',side_effect=ready), \
             patch.object(cli.approval_gate,'inspect',return_value={}) as audit:
            code,out,err = self.invoke(self.args())
        self.assertEqual((code,err),(0,''))
        self.assertEqual(json.loads(out),report)
        self.assertEqual(events,['stage','ready','reap','clean'])
        audit.assert_called_once()

    def test_denials_before_stage(self):
        good = self.args()
        cases = [[], good[1:], ['serve-reviewed',*good[1:]], good+['--fault-point','before_wal'],
                 good+['--fault-occurrence=1'], good+['--start'],
                 good+['--fault-errno','EIO'], good+['--fault-command','Unknown'],
                 good+['--worker-inputs'], good+['--enable-storage-fault'],
                 [x for x in good if x != '--enable-f14-prepare'],
                 [x for x in good if x != '--acknowledge-unproven-space']]
        for name,values in {'--fault-occurrence':['0','1025','01','+1','١'],

                            '--fault-evidence-root':['relative','/x/../y']}.items():
            for value in values:
                args=good.copy(); args[args.index(name)+1]=value; cases.append(args)
        with patch.object(cli,'stage') as stage:
            for args in cases:
                self.assertEqual(self.invoke(args),(2,'','LOCAL_F14_CHECK_REJECTED\n'))
            stage.assert_not_called()

    def test_error_interrupt_or_cleanup_failure_no_success(self):
        for error in (ValueError('SECRET'), OSError('/private/key'), KeyboardInterrupt()):
            @contextlib.contextmanager
            def stage(*args):
                yield 'staged'
                raise error
            @contextlib.contextmanager
            def ready(*args,**kwargs):
                yield dict(ready=True,fault_started=False, F14_verified=False,service_started=False,
                           approval_verified=False,reusable_permit=False)
            with patch.object(cli,'stage',side_effect=stage), patch.object(cli,'ready',side_effect=ready):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_F14_CHECK_REJECTED\n'))

    def test_report_types_and_missing_fields_rejected(self):
        good = dict(ready=True, fault_started=False, F14_verified=False, service_started=False,
                    approval_verified=False, reusable_permit=False)
        cases = [dict(good, ready=1), dict(good, fault_started=0),
                 dict(good, extra=False), {k:v for k,v in good.items() if k != 'ready'}]
        for report in cases:
            with patch.object(cli, 'stage', return_value=contextlib.nullcontext('staged')), \
                 patch.object(cli, 'ready', return_value=contextlib.nullcontext(report)):
                self.assertEqual(self.invoke(self.args()),
                                 (2,'','LOCAL_F14_CHECK_REJECTED\n'))

    def test_open_stdin_denial(self):
        proc=subprocess.Popen([sys.executable,'-B',cli.__file__,'check-ready-reviewed'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(proc.wait(timeout=3),2)
            self.assertEqual(proc.stdout.read(),b'')
            self.assertEqual(proc.stderr.read(),b'LOCAL_F14_CHECK_REJECTED\n')
        finally:
            if proc.poll() is None: proc.kill()
            proc.wait()
            for stream in (proc.stdin,proc.stdout,proc.stderr): stream.close()
