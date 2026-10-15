import json
from pathlib import Path
import subprocess
import sys
import unittest

import runtime_config
import test_reviewed_cli


class RuntimeConfigTest(unittest.TestCase):
    def args(self):
        return [*test_reviewed_cli.ReviewedCliTest.args(self), '--approval-socket', '/private/broker/s', '--pid-mailbox', '/private/session/pids']

    def test_real_shell_keeps_hostile_values_literal(self):
        args = ["space name", "quote'\"name", '$(exit 91)', '`exit 92`',
                '; exit 93', '$PAPERCLIP_API_KEY', '*', '\\value']
        command = runtime_config.shell_command([sys.executable, '-c',
            'import json,sys; print(json.dumps(sys.argv[1:]))', *args])
        for shell in ('/bin/sh', '/bin/zsh'):
            result = subprocess.run([shell, '-c', command], capture_output=True,
                                    text=True, timeout=3, env={'PATH':'/usr/bin:/bin'})
            self.assertEqual(result.returncode, 0)
            self.assertEqual(json.loads(result.stdout), args)
            self.assertEqual(result.stderr, '')

    def test_manual_fee_configs_keep_exact_argv(self):
        import shlex
        for fee in (0, 25):
            config = runtime_config.worker_config(sys.executable, '/candidate', self.args(), fee_bps=fee)
            runtime = config['runtimeConfig']
            self.assertEqual(runtime['desiredState'], 'manual')
            self.assertEqual(runtime['serviceStates'], {'0':'manual'})
            command = runtime['workspaceRuntime']['commands'][0]
            self.assertEqual(command['id'], f's3-worker-fee{fee}')
            self.assertEqual(shlex.split(command['command'])[5:], self.args())
            self.assertNotIn('env', command)
            self.assertNotIn('expose', command)

    def test_rejects_nonlocal_unbounded_and_control_inputs(self):
        for field, value in [('bind','0.0.0.0:8080'), ('rpc','https://example.org'),
                ('bind','127.0.0.1:65536'),('runtime-pin','a'*40),
                ('lifetime-seconds','301'),('max-ticks','0'),('max-requests','10001'),
                ('home','/tmp/{{workspace.cwd}}'),('home','/tmp/a\nb')]:
            args = self.args(); args[args.index('--'+field)+1] = value
            with self.assertRaises(ValueError):
                runtime_config.worker_config(sys.executable, '/candidate', args, fee_bps=0)
        for fee in (True, 1, '25'):
            with self.assertRaises(ValueError):
                runtime_config.worker_config(sys.executable, '/candidate', self.args(), fee_bps=fee)


if __name__ == '__main__': unittest.main()
