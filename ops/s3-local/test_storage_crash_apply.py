import contextlib
import json
import unittest
from unittest.mock import patch
import storage_crash_cli as cli
import storage_crash_run as runner
import storage_crash_ready as ready
import test_storage_crash_run as runs
import test_storage_crash_run_cli as clis

class ApplyCliTest(unittest.TestCase):
    invoke = clis.RunCliTest.invoke
    def test_explicit_apply_exact_route_and_result(self):
        args=clis.RunCliTest.args(self)+['--fault-command','Apply']
        report=clis.RunCliTest.report(self,0)
        report['child_result']['schema']='s3-local-crash-apply-result/1'
        for result in (report,clis.RunCliTest.report(self,0)):
            with patch.object(cli,'stage',return_value=contextlib.nullcontext('s')), patch.object(cli,'run',return_value=result) as run:
                code,out,_=self.invoke(args)
            self.assertEqual(run.call_args.kwargs['fault_command'],'Apply')
            self.assertEqual(code,0 if result is report else 2)
            if code: self.assertEqual(out,'')
        for extra in (['--fault-command','Apply','--fault-command','Seal'],['--fault-command','apply']):
            with patch.object(cli,'stage') as stage:
                self.assertEqual(self.invoke(clis.RunCliTest.args(self)+extra)[0],2)
                stage.assert_not_called()
        with self.assertRaises(ValueError):
            ready.crash_arguments('before_wal',1,'RESOLVE_FAILURE','/private/evidence',True,'Apply')

class ApplyRunTest(unittest.TestCase):
    setUp=runs.CrashRunTest.setUp
    worker=runs.CrashRunTest.worker
    reaped=runs.CrashRunTest.reaped
    def test_apply_subprocess_command_schema_and_seal_response_rejection(self):
        for schema in ('apply','seal'):
            obj=dict(schema='s3-local-crash-'+schema+'-result/1',command_succeeded=True,
                     crash_reached=False,crash_verified=False,durable_ack=False,DEV='NOT_RUN')
            raw=(json.dumps(obj,sort_keys=True,separators=(',',':'))+'\n').encode()
            child=self.worker('assert sys.argv[1]=="crash-apply-captured"\n'
                'assert "--worker-inputs" in sys.argv\n'
                's.sendall(b"READY\\n")\nassert s.recv(16)==b"START\\n"\nassert s.recv(1)==b""\n'
                'sys.stdout.buffer.write('+repr(raw)+')\n')
            def call():
                return runner.run(child,[],lambda:'same',point='before_wal',occurrence=1,
                    purpose='NORMAL',evidence_root=self.root/'evidence',enable=True,fault_command='Apply',timeout=2)
            if schema=='apply': self.assertEqual(call()['child_result'],obj)
            else:
                with self.assertRaisesRegex(ValueError,'OUTCOME_UNKNOWN'): call()
            self.reaped()
