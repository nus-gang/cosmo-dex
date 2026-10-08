import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import bootstrap_fetch_cli as cli
from fetch_process import FetchFailure
import test_bootstrap_cli as fixtures


class FetchCliTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.args = fixtures.BootstrapCliTest.args(self) + ['--chain-rpc','127.0.0.1:26657', '--evidence-root',str(self.root)]

    def invoke(self, args=None):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = cli.main(self.args if args is None else args)
        return code, out.getvalue(), err.getvalue()

    def test_success_exact_bytes_and_no_replace(self):
        with patch.object(cli,'fetch',return_value=b'raw\x00bytes') as fetch:
            code,out,err = self.invoke()
            self.assertEqual((code,err),(0,''))
            self.assertFalse(json.loads(out)['snapshot_verified'])
            self.assertEqual(fetch.call_args.args[-2], '127.0.0.1:26657')
            self.assertEqual(self.invoke(),(2,'','LOCAL_BOOTSTRAP_FETCH_REJECTED\n'))
            self.assertEqual(fetch.call_count,1)
        target=self.root/'snapshot-fetch.raw'
        self.assertEqual(target.read_bytes(),b'raw\x00bytes')
        self.assertEqual(target.stat().st_mode & 0o777,0o600)

    def test_partial_failure_and_interrupt_preserve_evidence(self):
        for error, expected in [(FetchFailure('SECRET',b'partial'),b'partial'),(KeyboardInterrupt(),b'')]:
            with self.subTest(error=type(error)), patch.object(cli,'fetch',side_effect=error):
                self.assertEqual(self.invoke(),(2,'','LOCAL_BOOTSTRAP_FETCH_REJECTED\n'))
                target=self.root/'snapshot-fetch.raw'
                self.assertEqual(target.read_bytes(),expected)
                target.unlink()

    def test_invalid_inputs_never_fetch(self):
        cases=[[],self.args[:-1],self.args+['--create'],self.args+['--chain-rpc','127.0.0.1:26658'],
               [x.replace('127.0.0.1:26657','0.0.0.0:26657') for x in self.args],
               [x.replace('127.0.0.1:26657','127.0.0.1:026657') for x in self.args]]
        with patch.object(cli,'fetch') as fetch:
            for args in cases:
                self.assertEqual(self.invoke(args),(2,'','LOCAL_BOOTSTRAP_FETCH_REJECTED\n'))
            self.root.chmod(0o755)
            self.assertEqual(self.invoke()[0],2)
            self.root.chmod(0o700)
            (self.root/'snapshot-fetch.raw').symlink_to(self.root/'absent')
            self.assertEqual(self.invoke()[0],2)
            fetch.assert_not_called()

    def test_fsync_failure_before_fetch_and_path_swap(self):
        with patch.object(cli.os,'fsync',side_effect=OSError('SECRET')),patch.object(cli,'fetch') as fetch:
            self.assertEqual(self.invoke()[0],2)
            fetch.assert_not_called()
        (self.root/'snapshot-fetch.raw').unlink()
        moved=self.root/'saved'
        def swap(*args,**kwargs):
            moved.mkdir()
            (self.root/'snapshot-fetch.raw').rename(moved/'raw')
            # Change the root mode: final identity check must reject, retaining bytes.
            self.root.chmod(0o750)
            return b'evidence'
        with patch.object(cli,'fetch',side_effect=swap):
            self.assertEqual(self.invoke()[0],2)
        self.assertEqual((moved/'raw').read_bytes(),b'evidence')
        self.root.chmod(0o700)

    def test_open_stdin_rejection(self):
        child=subprocess.Popen([sys.executable,'-B',cli.__file__,'--create'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(child.wait(timeout=3),2)
            self.assertEqual(child.stdout.read(),b'')
        finally:
            if child.poll() is None: child.kill()
            child.wait()
            for stream in (child.stdin,child.stdout,child.stderr): stream.close()
