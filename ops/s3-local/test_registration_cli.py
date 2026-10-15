import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

import registration_cli as target
import test_runtime_config
from workspace_registration import prepare


class RegistrationCliTest(unittest.TestCase):
    def args(self, fee='0'):
        return ['packet', '--python', sys.executable, '--candidate', '/candidate',
                '--fee-bps', fee, '--', *test_runtime_config.RuntimeConfigTest.args(self)]

    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = target.main(args)
        return rc, out.getvalue(), err.getvalue()

    def test_exact_packet_both_fees_deterministic_no_effects(self):
        for fee in ('0', '25'):
            args = self.args(fee)
            expected = prepare(sys.executable, '/candidate', args[8:], fee_bps=int(fee))
            with patch('pathlib.Path.mkdir', side_effect=AssertionError('filesystem mutation')), \
                 patch('subprocess.Popen', side_effect=AssertionError('spawn')):
                result = self.invoke(args)
                self.assertEqual(result, self.invoke(args))
            self.assertEqual(result[0], 0)
            self.assertEqual(result[2], '')
            self.assertEqual(json.loads(result[1]), expected)
            self.assertFalse(expected['approval_verified'])
            self.assertFalse(expected['starts_service'])
            self.assertTrue(expected['requires_board_registration'])

    def test_envelope_and_worker_denials_output_nothing(self):
        valid = self.args()
        cases = [[], ['serve', *valid[1:]], valid[:7],
                 ['packet', '--py', *valid[2:]], self.args('025'), self.args('1'),
                 [*valid, '--unknown'], [*valid, '--runtime-pin', 'a'*64]]
        for index, value in ((2, 'python3'), (4, '/candidate/../other'), (7, '--extra')):
            args = valid.copy(); args[index] = value; cases.append(args)
        for flag in ('--local-demo-profile', '--acknowledge-unproven-space'):
            args = valid.copy(); index = args.index(flag)
            del args[index:index+(2 if flag == '--local-demo-profile' else 1)]
            cases.append(args)
        for args in cases:
            with self.subTest(args=args[:8]):
                self.assertEqual(self.invoke(args), (2, '', target.ERROR+'\n'))

    def test_internal_errors_do_not_print_details(self):
        for error in (OSError('TOKEN_SECRET'), ValueError('TOKEN_SECRET'), KeyboardInterrupt()):
            with patch.object(target, 'prepare', side_effect=error):
                self.assertEqual(self.invoke(self.args()), (2, '', target.ERROR+'\n'))

    def test_bad_cli_does_not_wait_for_stdin(self):
        p = subprocess.Popen([sys.executable, '-B', str(Path(target.__file__)), 'serve'],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            self.assertEqual(p.wait(timeout=3), 2)
            self.assertEqual(p.stdout.read(), b'')
            self.assertEqual(p.stderr.read(), (target.ERROR+'\n').encode())
        finally:
            if p.poll() is None: p.kill(); p.wait()
            p.stdin.close(); p.stdout.close(); p.stderr.close()


if __name__ == '__main__': unittest.main()
