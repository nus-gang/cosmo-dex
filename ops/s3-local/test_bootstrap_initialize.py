from contextlib import contextmanager, ExitStack, redirect_stdout, redirect_stderr
import io
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
import bootstrap_initialize as init
import bootstrap_initialize_cli as cli
import test_bootstrap_cli as fixtures
from fetch_process import FetchFailure


class InitializeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.argv = fixtures.BootstrapCliTest.args(self) + [
            '--chain-rpc','127.0.0.1:26657','--evidence-root',str(self.root),
            '--home',str(self.root/'new-home')]
        self.args = cli.parse(self.argv)
        self.events = []
        self.snapshot = b'capture'
        self.approval = {'decision':'approved fixture'}
        self.stop = False
        self.stage_capture = b'capture'
        self.raw = b'raw exact\x00snapshot'
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        self.audit = self.stack.enter_context(patch.object(init.approval_gate,'inspect',side_effect=lambda *a: dict(self.approval)))
        self.stack.enter_context(patch.object(init,'verify_input_set',side_effect=lambda *a: (self.snapshot,{})))
        self.fetch = self.stack.enter_context(patch.object(init,'execute_raw',side_effect=self.fetch_raw))
        self.stage = self.stack.enter_context(patch.object(init,'stage',side_effect=self.staging))
        self.run = self.stack.enter_context(patch.object(init,'run',side_effect=self.running))

    def fetch_raw(self,*args):
        self.events.append('fetch-preserve')
        (self.root/'snapshot-fetch.raw').write_bytes(self.raw)
        return {}, self.raw

    @contextmanager
    def staging(self,*args):
        self.events.append('stage')
        self.assertEqual(args[-2],self.raw)
        try:
            yield SimpleNamespace(capture=self.stage_capture)
        finally:
            self.events.append('stage-cleanup')

    def running(self,*args,**kwargs):
        self.events.append('run')
        self.assertEqual(args[-1](),self.approval)
        self.assertFalse(kwargs['stop']())
        return {'child_reported_home_created':True,'service_started':False,'replay_verified':False}

    def call(self):
        return init.initialize(*self.args,stopped=lambda:self.stop)

    def test_exact_bytes_order_and_no_file_reread(self):
        self.events.clear()
        def altered(*args):
            result=self.fetch_raw(*args)
            (self.root/'snapshot-fetch.raw').write_bytes(b'replaced path')
            return result
        self.fetch.side_effect=altered
        self.assertTrue(self.call()['child_reported_home_created'])
        self.assertEqual(self.events,['fetch-preserve','stage','run','stage-cleanup'])
        self.assertFalse((self.root/'new-home').exists()) # synthetic runner

    def test_fetch_failure_stop_and_changed_inputs_never_create(self):
        for kind in ('partial','approval','input','stop'):
            with self.subTest(kind=kind):
                self.snapshot=b'capture'; self.approval={'decision':'approved fixture'}; self.stop=False
                self.stage.reset_mock(); self.run.reset_mock()
                def changed(*args):
                    result=self.fetch_raw(*args)
                    if kind=='partial': raise FetchFailure('private',self.raw)
                    if kind=='approval': self.approval={'decision':'revoked'}
                    if kind=='input': self.snapshot=b'changed'
                    if kind=='stop': self.stop=True
                    return result
                self.fetch.side_effect=changed
                with self.assertRaises((ValueError,FetchFailure)): self.call()
                self.stage.assert_not_called(); self.run.assert_not_called()
                self.assertEqual((self.root/'snapshot-fetch.raw').read_bytes(),self.raw)

    def test_stage_mismatch_and_run_failure_cleanup_no_retry(self):
        self.stage_capture=b'other'
        with self.assertRaisesRegex(ValueError,'CAPTURE_CHANGED'): self.call()
        self.run.assert_not_called()
        self.assertEqual(self.events[-1],'stage-cleanup')
        self.stage_capture=b'capture'; self.events.clear(); self.fetch.reset_mock()
        self.run.side_effect=KeyboardInterrupt
        with self.assertRaises(KeyboardInterrupt): self.call()
        self.assertEqual(self.fetch.call_count,1)
        self.assertEqual(self.run.call_count,1)
        self.assertEqual(self.events[-1],'stage-cleanup')
        self.assertTrue((self.root/'snapshot-fetch.raw').exists())

    def test_cli_and_existing_home_rejections(self):
        for argv in ([], self.argv+['--home','/other'], self.argv+['--serve'],
                     [v.replace(str(self.root/'new-home'),'relative') for v in self.argv]):
            out,err=io.StringIO(),io.StringIO()
            with redirect_stdout(out),redirect_stderr(err): self.assertEqual(cli.main(argv),2)
            self.assertEqual(out.getvalue(),'')
            self.assertEqual(err.getvalue(),'LOCAL_INITIALIZE_REJECTED_PRESERVE_HOME_AND_EVIDENCE\n')
        (self.root/'new-home').mkdir()
        with self.assertRaisesRegex(ValueError,'NEW_HOME_REQUIRED'): self.call()
        self.fetch.assert_not_called()
        with patch.object(cli,'initialize',return_value={'service_started':False}) as runner:
            with redirect_stdout(io.StringIO()): self.assertEqual(cli.main(self.argv),0)
            self.assertEqual(runner.call_args.args,self.args)
