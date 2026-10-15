import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
import private_reader as api

class FixedBrokerTest(unittest.TestCase):
    def setUp(self):
        self.parent = Path(os.environ['PAPERCLIP_RUN_SCRATCH_DIR']).resolve()
        self.root = self.parent / 'f'
        self.assertFalse(self.root.exists())
        self.addCleanup(self.cleanup)

    def cleanup(self):
        if self.root.exists():
            for p in self.root.iterdir(): p.unlink()
            self.root.rmdir()

    def test_fixed_path_fresh_reader_reuse_and_duplicate_refusal(self):
        path = sorted(api.PATHS)[0]
        for n in (1, 2):
            reader = Mock(return_value={'revision': n})
            with patch.object(api.Reader, 'from_environment', return_value=reader):
                with api.broker_at(self.root) as endpoint:
                    self.assertEqual(endpoint, self.root / 's')
                    self.assertEqual(api.PrivateReader(endpoint)(path), {'revision': n})
                    with self.assertRaises(FileExistsError):
                        with api.broker_at(self.root): pass
                    self.assertEqual(api.PrivateReader(endpoint)(path), {'revision': n})
            self.assertFalse(self.root.exists())

    def test_stale_file_directory_link_and_nonprivate_parent_preserved(self):
        for kind in ('directory','file','link'):
            if kind == 'directory': self.root.mkdir()
            elif kind == 'file': self.root.write_text('evidence')
            else: self.root.symlink_to(self.parent)
            with self.assertRaises(FileExistsError):
                with api._broker(None, Mock(), fixed_root=self.root): pass
            if kind == 'directory': self.root.rmdir()
            else: self.root.unlink()
        meta = self.parent.lstat()
        bad = Mock(st_mode=0o40755, st_uid=meta.st_uid)
        with patch.object(Path, 'lstat', return_value=bad):
            with self.assertRaises(ValueError):
                with api._broker(None, Mock(), fixed_root=self.root): pass
        self.assertFalse(self.root.exists())

    def test_failure_interrupt_and_unexpected_evidence_cleanup(self):
        with patch.object(api.socket, 'socket', side_effect=OSError('private')):
            with self.assertRaises(OSError):
                with api._broker(None, Mock(), fixed_root=self.root): pass
        self.assertFalse(self.root.exists())
        with self.assertRaises(KeyboardInterrupt):
            with api._broker(None, Mock(), fixed_root=self.root):
                raise KeyboardInterrupt()
        self.assertFalse(self.root.exists())
        with self.assertRaises(OSError):
            with api._broker(None, Mock(), fixed_root=self.root):
                (self.root / 'evidence').write_text('preserve')
        self.assertEqual((self.root / 'evidence').read_text(), 'preserve')
        self.assertFalse((self.root / 's').exists())

if __name__ == '__main__': unittest.main()
