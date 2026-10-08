"""Built crash child rejection only: no START, service or RPC."""
import os
import subprocess
import unittest

class CrashChildDenials(unittest.TestCase):
    def test_bad_cli_does_not_wait_for_stdin(self):
        binary = os.environ['S3_CRASH_CHILD_BINARY']
        prefix = ['crash-seal-captured', '--start-gate-fd', '3',
                  '--capture-sha256', 'a' * 64]
        opts = ['--enable-storage-crash', '--fault-point', 'before_wal',
                '--fault-occurrence', '1', '--fault-purpose', 'NORMAL',
                '--fault-evidence-root', '/private/unused-evidence']
        cases = [[], ['fault-seal-captured'], ['crash-apply-captured'],
                 prefix[:2] + ['0'], prefix[:2] + ['03'],
                 prefix + opts, prefix + opts + ['--worker-inputs'],
                 prefix + opts + ['--fault-errno', 'EIO', '--worker-inputs'],
                 prefix + ['--enable-storage-fault'] + opts[1:] + ['--worker-inputs']]
        for args in cases:
            with self.subTest(args=args):
                p = subprocess.Popen([binary, *args], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    self.assertEqual(p.wait(timeout=2), 2)
                    self.assertEqual(p.stdout.read(), b'')
                    self.assertEqual(p.stderr.read(), b'LOCAL_STORAGE_CRASH_REJECTED\n')
                finally:
                    if p.poll() is None:
                        p.kill()
                    p.wait()
                    for stream in (p.stdin, p.stdout, p.stderr):
                        stream.close()

if __name__ == '__main__':
    unittest.main()
