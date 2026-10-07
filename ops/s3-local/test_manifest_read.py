"""Deterministic filesystem races; no services, keys or approved runtime."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import manifest as m


class ManifestRead(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.file = self.root / 'binary'
        self.file.write_bytes(b'original')

    def test_replacement_after_path_check_never_consumes_link_or_fifo(self):
        original = os.open
        for kind in ('link', 'fifo', 'hardlink'):
            self.file.unlink(missing_ok=True)
            self.file.write_bytes(b'original')
            target = self.root / 'target'
            target.write_bytes(b'outside')
            def opening(path, flags, *args, **kwargs):
                if path == 'binary':
                    self.file.unlink()
                    if kind == 'link':
                        self.file.symlink_to(target)
                    elif kind == 'fifo':
                        os.mkfifo(self.file)
                    else:
                        os.link(target, self.file)
                return original(path, flags, *args, **kwargs)
            with self.subTest(kind=kind), patch.object(m.os, 'open', side_effect=opening):
                with self.assertRaises((OSError, ValueError)):
                    m.read_regular(self.root, 'binary')

    def test_mutation_or_inode_replacement_during_read_is_rejected(self):
        original = os.read
        for kind in ('overwrite', 'replace', 'grow'):
            self.file.write_bytes(b'original')
            calls = 0
            def reading(fd, size):
                nonlocal calls
                raw = original(fd, size)
                calls += 1
                if calls == 1:
                    if kind == 'replace':
                        self.file.unlink()
                        self.file.write_bytes(b'original')
                    else:
                        self.file.write_bytes(b'modified' if kind == 'overwrite' else b'original-longer')
                return raw
            with self.subTest(kind=kind), patch.object(m.os, 'read', side_effect=reading):
                with self.assertRaisesRegex(ValueError, 'FILE_CHANGED_DURING_READ'):
                    m.read_regular(self.root, 'binary')

    def test_directory_replacement_with_link_before_open_is_rejected(self):
        directory = self.root / 'sub'
        directory.mkdir()
        (directory / 'binary').write_bytes(b'inside')
        original = os.open
        def opening(path, flags, *args, **kwargs):
            if path == 'sub':
                directory.rename(self.root / 'saved')
                directory.symlink_to(self.root / 'saved', target_is_directory=True)
            return original(path, flags, *args, **kwargs)
        with patch.object(m.os, 'open', side_effect=opening):
            with self.assertRaises(OSError):
                m.read_regular(self.root, 'sub/binary')

    def test_unchanged_empty_and_large_bytes_and_error_descriptor_cleanup(self):
        for raw in (b'', b'exact', b'x' * (1048576 + 13)):
            self.file.write_bytes(raw)
            self.assertEqual(m.read_regular(self.root, 'binary'), raw)
        original = os.close
        closed = []
        def closing(fd):
            closed.append(fd)
            original(fd)
        with patch.object(m.os, 'read', side_effect=OSError('injected')), patch.object(m.os, 'close', side_effect=closing):
            with self.assertRaises(OSError):
                m.read_regular(self.root, 'binary')
        self.assertEqual(len(closed), 2)
        for fd in closed:
            with self.assertRaises(OSError):
                os.fstat(fd)


if __name__ == '__main__':
    unittest.main()
