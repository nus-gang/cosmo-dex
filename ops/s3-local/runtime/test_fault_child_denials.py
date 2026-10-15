"""Offline denial checks for the real, separately built storage fault child."""
import os
import subprocess
import unittest


class FaultChildDenials(unittest.TestCase):
    def test_invalid_arguments_reject_without_reading_open_stdin(self):
        executable = os.environ['NUS73_FAULT_CHILD_EXECUTABLE']
        prefix = ['fault-seal-captured', '--start-gate-fd', '3', '--capture-sha256', 'a' * 64]
        fault = ['--enable-storage-fault', '--fault-point', 'before_wal', '--fault-occurrence', '1',
                 '--fault-purpose', 'NORMAL', '--fault-evidence-root', '/private/unused-evidence']
        worker = ['--input-set', '/unused-input', '--local-demo-profile', '/unused-profile',
                  '--runtime-pin', 'a' * 64, '--home', '/unused-home', '--key-directory', '/unused-keys',
                  '--bind', '127.0.0.1:18080', '--rpc', '127.0.0.1:26657',
                  '--lifetime-seconds', '60', '--max-requests', '1', '--max-ticks', '1',
                  '--acknowledge-unproven-space']
        bad_number = fault.copy()
        bad_number[4] = '01'
        cases = [[], ['serve-captured'], prefix + fault[1:] + worker,
                 prefix + bad_number + worker, prefix + fault + worker[:-1],
                 prefix + fault + worker + ['--enable-storage-fault'],
                 prefix + fault + worker]  # fd 3 is not inherited: no input read
        for value in ('ENOSPC','EDQUOT','EIO','enospc','28'):
            cases.append(prefix + fault + ['--fault-errno', value] + worker)
        cases.append(prefix + fault + ['--fault-errno','EIO','--fault-errno','EIO'] + worker)
        for argv in cases:
            with self.subTest(argv=argv):
                child = subprocess.Popen([executable, *argv], stdin=subprocess.PIPE,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                         env={}, close_fds=True)
                try:
                    self.assertEqual(child.wait(timeout=3), 2)
                    self.assertEqual(child.stdout.read(), b'')
                    self.assertEqual(child.stderr.read(), b'LOCAL_STORAGE_FAULT_REJECTED\n')
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=3)
                    for stream in (child.stdin, child.stdout, child.stderr):
                        stream.close()


if __name__ == '__main__':
    unittest.main()
