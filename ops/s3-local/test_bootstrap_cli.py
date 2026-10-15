import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch
import bootstrap_cli
import test_bootstrap_check as fixtures


class BootstrapCliTest(unittest.TestCase):
    def args(self):
        args = []
        for key, value in {'bundle':'/bundle', 'artifacts':'/artifacts',
            'input-set':'/inputs/input.json', 'effective-profile':'/profile.json',
            'scratch':'/scratch', 'runtime-pin':'a'*64,
            'local-demo-profile':'s3-dev-local/1',
            'native-decision-id':'00000000-0000-4000-8000-000000000001',
            'ceo-revision':'00000000-0000-4000-8000-000000000002',
            'cto-revision':'00000000-0000-4000-8000-000000000003'}.items():
            args.extend(['--'+key,value])
        return args + ['--acknowledge-unproven-space']

    def invoke(self,args):
        out,err=io.StringIO(),io.StringIO()
        with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
            code=bootstrap_cli.main(args)
        return code,out.getvalue(),err.getvalue()

    def test_arguments_and_fixed_rejection_before_io(self):
        good=self.args()
        with patch.object(bootstrap_cli,'check',return_value={'home_created':False}) as check:
            self.assertEqual(self.invoke(good),(0,'{"home_created": false}\n',''))
            self.assertEqual(check.call_args.args[7:],(Path('/inputs'),'input.json',Path('/profile.json'),Path('/scratch')))
        cases=[[],good[:-1],good+['--serve'],good+['--home','/new'],
            good+['--runtime-pin','b'*64],good+['--help'],
            [x.replace('--runtime-pin','--runtime-p') for x in good],
            [x.replace('/profile.json','relative') for x in good],
            [x.replace('a'*64,'A'*64) for x in good],
            [x.replace('00000000-0000-4000-8000-000000000001','bad') for x in good]]
        with patch.object(bootstrap_cli,'check') as check:
            for args in cases:
                self.assertEqual(self.invoke(args),(2,'','LOCAL_BOOTSTRAP_CHECK_REJECTED\n'))
            check.assert_not_called()

    def test_errors_do_not_expose_diagnostics(self):
        for error in (ValueError('SECRET'),OSError('/private'),KeyboardInterrupt()):
            with patch.object(bootstrap_cli,'check',side_effect=error):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_BOOTSTRAP_CHECK_REJECTED\n'))

    def test_open_stdin_invalid_cli_exits(self):
        child=subprocess.Popen([sys.executable,'-B',bootstrap_cli.__file__,'--create'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(child.wait(timeout=3),2)
            self.assertEqual(child.stdout.read(),b'')
            self.assertEqual(child.stderr.read(),b'LOCAL_BOOTSTRAP_CHECK_REJECTED\n')
        finally:
            if child.poll() is None: child.kill()
            child.wait()
            for stream in (child.stdin,child.stdout,child.stderr): stream.close()


class BootstrapCliIntegration(fixtures.BootstrapCheckTest):
    def test_cli_capture_checker_and_reaudit(self):
        self.prepare()
        args=BootstrapCliTest.args(self)
        replacements={'/bundle':str(self.bundle),'/artifacts':str(self.artifacts),
            '/inputs/input.json':str(self.input_file),'/profile.json':str(self.root/'profile.json'),
            '/scratch':str(self.scratch),'a'*64:self.pin}
        args=[replacements.get(x,x) for x in args]
        with patch.object(fixtures.bootstrap_check.approval_gate,'inspect',return_value={'v':1}) as audit:
            code,out,err=BootstrapCliTest.invoke(self,args)
        self.assertEqual((code,err),(0,''))
        result=json.loads(out)
        self.assertFalse(result['home_created'])
        self.assertFalse(result['services_started'])
        self.assertFalse(result['reusable_permit'])
        self.assertEqual(audit.call_count,2)
        self.assertEqual(list(self.scratch.iterdir()),[])
