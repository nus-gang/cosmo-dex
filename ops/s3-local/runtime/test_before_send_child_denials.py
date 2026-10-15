"""Real F05 child invalid-input checks; no START, RPC or transport."""
import os
import subprocess
import unittest

class BeforeSendChildDenials(unittest.TestCase):
    def test_rejects_before_reading_open_stdin(self):
        executable = os.environ['NUS73_F05_CHILD_EXECUTABLE']
        prefix = ['f05-crash-captured', '--start-gate-fd', '3', '--capture-sha256', 'a' * 64]
        fault = ['--enable-f05-before-send', 'true', '--tx-hash', 'b' * 64,
                 '--fault-evidence-root', '/private/unused-evidence', '--worker-inputs']
        worker = ['--input-set', '/unused-input', '--local-demo-profile', '/unused-profile',
                  '--runtime-pin', 'a' * 64, '--home', '/unused-home', '--key-directory', '/unused-keys',
                  '--bind', '127.0.0.1:18080', '--rpc', '127.0.0.1:26657',
                  '--lifetime-seconds', '60', '--max-requests', '1', '--max-ticks', '1',
                  '--acknowledge-unproven-space']
        cases = [[], ['serve-captured'], prefix + fault + worker, prefix + fault + worker[:-1]]
        for index, value in [(1, 'false'), (3, 'B' * 64), (5, '/private/../evidence'),
                             (6, '--fault-errno')]:
            bad = fault.copy()
            bad[index] = value
            cases.append(prefix + bad + worker)
        cases.append(prefix + fault + worker + ['--fault-command', 'Apply'])
        for argv in cases:
            with self.subTest(argv=argv):
                child = subprocess.Popen([executable, *argv], stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, env={}, close_fds=True)
                try:
                    self.assertEqual(child.wait(timeout=3), 2)
                    self.assertEqual(child.stdout.read(), b'')
                    self.assertEqual(child.stderr.read(), b'LOCAL_F05_REJECTED\n')
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=3)
                    for stream in (child.stdin, child.stdout, child.stderr):
                        stream.close()

if __name__ == '__main__':
    unittest.main()
