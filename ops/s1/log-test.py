#!/usr/bin/env python3
"""Real file/pipe rotation and injected I/O failures; no network or validator keys."""
import errno
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from bounded_log import BACKUPS, MAX_BYTES, BoundedLog, LogPump
import devnet


class LogTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.path = self.root / 'node0.log'

    def tearDown(self):
        self.temp.cleanup()

    def retained(self):
        return b''.join(p.read_bytes() for p in
                       [self.path.with_name('node0.log.' + str(i)) for i in range(4, 0, -1)] + [self.path]
                       if p.exists())

    def test_retains_exact_suffix_and_count_across_reopen(self):
        data = bytes(range(256)) * 41 + b'ERROR: latest stderr\n'
        log = BoundedLog(self.path, 1024)
        log.write(data[:2500]); log.close()
        log = BoundedLog(self.path, 1024)
        log.write(data[2500:]); log.close()
        files = list(self.root.iterdir())
        self.assertEqual(len(files), 5)
        self.assertTrue(all(p.stat().st_size <= 1024 for p in files))
        expected_size = 4 * 1024 + ((len(data) - 1) % 1024 + 1)
        self.assertEqual(self.retained(), data[-expected_size:])

    def test_real_default_100mib_boundary(self):
        self.assertEqual((MAX_BYTES, BACKUPS), (104857600, 4))
        log = BoundedLog(self.path)
        block = b'x' * (1024 * 1024)
        for _ in range(100):
            log.write(block)
        log.write(b'ERROR after boundary\n'); log.close()
        self.assertEqual(log.backup(1).stat().st_size, MAX_BYTES)
        self.assertEqual(self.path.read_bytes(), b'ERROR after boundary\n')
        self.assertEqual(len(list(self.root.iterdir())), 2)

    def test_stdout_stderr_and_unterminated_large_record(self):
        code = "import os; os.write(1,b'x'*150000); os.write(2,b'ERROR stderr\\n')"
        proc = subprocess.Popen([sys.executable, '-c', code], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        pump = LogPump(proc.stdout, BoundedLog(self.path, 65536))
        self.assertEqual(proc.wait(timeout=5), 0)
        pump.finish()
        self.assertEqual(self.retained(), b'x' * 150000 + b'ERROR stderr\n')

    def test_oversized_legacy_log_refused_without_truncation(self):
        self.path.write_bytes(b'evidence' * 20)
        with self.assertRaisesRegex(RuntimeError, 'oversized'):
            BoundedLog(self.path, 100)
        self.assertEqual(self.path.read_bytes(), b'evidence' * 20)

    def test_partial_write_and_zero_progress(self):
        log = BoundedLog(self.path, 16)
        original = log.file
        class ShortWriter:
            def write(self, data):
                return original.write(data[:2])
            def close(self):
                original.close()
        log.file = ShortWriter()
        log.write(b'abcdefgh')
        self.assertEqual(self.path.read_bytes(), b'abcdefgh')
        with patch.object(log.file, 'write', return_value=0):
            with self.assertRaises(OSError):
                log.write(b'x')
        log.close()

    def test_rotation_rename_failure_preserves_active_error(self):
        log = BoundedLog(self.path, 5)
        log.write(b'ERROR')
        with patch.object(Path, 'replace', side_effect=OSError(errno.EACCES, 'denied')):
            with self.assertRaises(OSError):
                log.write(b'x')
        log.close()
        self.assertEqual(self.path.read_bytes(), b'ERROR')

    def test_disk_full_propagates_from_pump(self):
        proc = subprocess.Popen([sys.executable, '-c', "print('ERROR')"], stdout=subprocess.PIPE)
        sink = BoundedLog(self.path)
        with patch.object(sink, 'write', side_effect=OSError(errno.ENOSPC, 'disk full')):
            pump = LogPump(proc.stdout, sink)
            proc.wait(timeout=5)
            with self.assertRaisesRegex(RuntimeError, 'disk full'):
                pump.finish()

    def test_supervisor_reaps_all_children_on_log_failure(self):
        # Actual child processes, fake manifest only. No chain data or RPC ports.
        child = self.root / 'writer.py'
        child.write_text('#!' + sys.executable + '\nimport time\nprint("ERROR",flush=True)\ntime.sleep(60)\n')
        child.chmod(0o700)
        manifest = {'binary': str(child), 'genesis_sha256': 'test',
                    'nodes': [{'home': str(self.root)} for _ in range(4)]}
        children = []
        real_popen = subprocess.Popen
        def spawn(*args, **kwargs):
            p = real_popen(*args, **kwargs); children.append(p); return p
        cwd = Path.cwd()
        old_umask = os.umask(0o077)
        try:
            with patch.object(devnet, 'load', return_value=manifest), \
                 patch.object(devnet.subprocess, 'Popen', side_effect=spawn), \
                 patch.object(BoundedLog, 'write', side_effect=OSError(errno.ENOSPC, 'disk full')), \
                 patch.object(devnet.signal, 'signal'):
                with self.assertRaisesRegex(RuntimeError, 'disk full'):
                    devnet.serve(self.root)
            self.assertEqual(len(children), 4)
            self.assertTrue(all(p.poll() is not None for p in children))
            self.assertFalse((self.root / '.control.sock').exists())
        finally:
            os.chdir(cwd)
            os.umask(old_umask)
            for p in children:
                if p.poll() is None:
                    p.kill(); p.wait()


if __name__ == '__main__':
    unittest.main(verbosity=2)
