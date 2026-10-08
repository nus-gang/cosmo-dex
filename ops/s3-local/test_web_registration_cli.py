import contextlib
import copy
import io
import json
from pathlib import Path
import shlex
import subprocess
import sys
import unittest
from unittest.mock import patch
import test_web_cli as fixtures
import web_registration_cli as target
import workspace_registration as registration
from native_review import COMPANY

class WebPacketTest(unittest.TestCase):
    def args(self, fee='0', fault=None):
        args = ['web-packet', '--python', sys.executable, '--candidate', '/candidate',
                '--fee-bps', fee, '--', *fixtures.WebCliTest.args(self)[1:]]
        if fault is not None:
            args += ['--drop-broadcast-response-sha256', fault, '--fault-evidence-root', '/private/fault-evidence']
        return args

    def invoke(self, args):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = target.main(args)
        return rc, out.getvalue(), err.getvalue()

    def test_default_and_fault_packets_exact_and_inert(self):
        for fee in ('0', '25'):
            for fault in (None, 'a'*64):
                args = self.args(fee, fault)
                with patch('subprocess.Popen', side_effect=AssertionError('spawn')), \
                     patch('pathlib.Path.mkdir', side_effect=AssertionError('mkdir')):
                    result = self.invoke(args)
                    self.assertEqual(result, self.invoke(args))
                self.assertEqual((result[0], result[2]), (0, ''))
                packet = json.loads(result[1])
                self.assertEqual(packet, registration.prepare_web(sys.executable,
                    '/candidate', args[8:], fee_bps=int(fee)))
                command = packet['body']['runtimeConfig']['workspaceRuntime']['commands'][0]
                self.assertEqual(shlex.split(command['command'])[5:], args[8:])
                self.assertFalse(packet['starts_service'])
                self.assertFalse(packet['approval_verified'])
                self.assertTrue(packet['requires_board_registration'])
                workspace = dict(copy.deepcopy(packet['body']),
                    id='11111111-1111-4111-8111-111111111111', companyId=COMPANY,
                    projectId=registration.PROJECT, runtimeServices=[])
                registration.registered(packet, [workspace])
                changed = json.loads(self.invoke(self.args(fee, 'b'*64))[1])
                with self.assertRaises(ValueError):
                    registration.registered(changed, [workspace])

    def test_invalid_fault_envelope_and_optins_leave_stdout_empty(self):
        cases = [[], self.args('025'), self.args(fault='A'*64),
                 self.args(fault='a'*63), ['packet', *self.args()[1:]],
                 self.args(fault='a'*64)+['--drop-broadcast-response-sha256','b'*64]]
        for option in ('--acknowledge-unproven-space', '--local-demo-profile'):
            args = self.args(fault='a'*64); at = args.index(option)
            del args[at:at+(2 if option == '--local-demo-profile' else 1)]
            cases.append(args)
        for args in cases:
            self.assertEqual(self.invoke(args), (2, '', 'LOCAL_REGISTRATION_PACKET_REJECTED\n'))

    def test_invalid_subprocess_does_not_read_stdin_and_errors_are_private(self):
        child = subprocess.Popen([sys.executable, '-B', target.__file__, 'serve'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            self.assertEqual(child.wait(timeout=3), 2)
            self.assertEqual(child.stdout.read(), b'')
            self.assertEqual(child.stderr.read(), b'LOCAL_REGISTRATION_PACKET_REJECTED\n')
        finally:
            if child.poll() is None: child.kill(); child.wait()
            for stream in (child.stdin, child.stdout, child.stderr): stream.close()
        with patch.object(target, 'prepare_web', side_effect=OSError('secret')):
            self.assertEqual(self.invoke(self.args()),
                (2, '', 'LOCAL_REGISTRATION_PACKET_REJECTED\n'))

if __name__ == '__main__': unittest.main()
