import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch
import offline_cli

class OfflineCliTest(unittest.TestCase):
    def args(self):
        values = dict(bundle='/bundle', artifacts='/artifacts', **{
            'input-set':'/inputs/input.json', 'effective-profile':'/inputs/profile.json',
            'home':'/home', 'key-directory':'/keys', 'scratch':'/scratch',
            'runtime-pin':'a'*64, 'local-demo-profile':'s3-dev-local/1',
            'bind':'127.0.0.1:18080', 'rpc':'127.0.0.1:26657',
            'lifetime-seconds':'60', 'max-requests':'10', 'max-ticks':'10'})
        return [v for k,x in values.items() for v in ('--'+k,x)] + ['--acknowledge-unproven-space']
    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = offline_cli.main(args)
        return code, out.getvalue(), err.getvalue()
    def test_explicit_mapping_and_success_report(self):
        report = {'approval_verified':False,'services_started':False,'DEV':'NOT_RUN'}
        with patch.object(offline_cli,'check',return_value=report) as check:
            code, out, err = self.invoke(self.args())
        self.assertEqual((code,err),(0,''))
        self.assertEqual(json.loads(out),report)
        args = check.call_args.args
        self.assertEqual(args[:3],(Path('/bundle'),Path('/artifacts'),'a'*64))
        self.assertEqual(args[5:7],(Path('/inputs'),'input.json'))
        forwarded=args[7]
        self.assertEqual(forwarded[forwarded.index('--local-demo-profile')+1],'/inputs/profile.json')
        self.assertIn('--acknowledge-unproven-space',forwarded)
        self.assertNotIn('--scratch',forwarded)
    def test_rejected_arguments_do_not_invoke_validator(self):
        good=self.args()
        cases=[[],good[:-1],good+['--runtime-pin','b'*64],good+['--serve'],
               [x.replace('/inputs/input.json','relative.json') for x in good],
               [x.replace('s3-dev-local/1','standard') for x in good],
               [x.replace('--bundle','--bund') for x in good]]
        with patch.object(offline_cli,'check') as check:
            for args in cases:
                self.assertEqual(self.invoke(args),(2,'','LOCAL_OFFLINE_CHECK_REJECTED\n'))
            check.assert_not_called()
    def test_errors_do_not_leak_or_report_success(self):
        for error in (ValueError('SECRET'),OSError('/private-key'),KeyboardInterrupt()):
            with patch.object(offline_cli,'check',side_effect=error):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_OFFLINE_CHECK_REJECTED\n'))
    def test_subprocess_rejection_without_stdin_eof(self):
        proc=subprocess.Popen([sys.executable,'-B',offline_cli.__file__,'--serve'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(proc.wait(timeout=3),2)
            self.assertEqual(proc.stdout.read(),b'')
            self.assertEqual(proc.stderr.read(),b'LOCAL_OFFLINE_CHECK_REJECTED\n')
        finally:
            if proc.poll() is None: proc.kill()
            proc.wait()
            for stream in (proc.stdin,proc.stdout,proc.stderr): stream.close()

if __name__=='__main__': unittest.main()
