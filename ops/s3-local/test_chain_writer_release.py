import fcntl
import os
from pathlib import Path
import tempfile
import unittest
import chain_writer_release as subject


class WriterTest(unittest.TestCase):
    def fixture(self):
        tmp = tempfile.TemporaryDirectory(); self.addCleanup(tmp.cleanup)
        root = Path(tmp.name).resolve(); root.chmod(0o700)
        p = root / subject.NAME; p.write_bytes(b'preserved'); p.chmod(0o600)
        return root, p

    def test_existing_lock_reacquired_without_file_changes(self):
        for fee in (0, 25):
            root, p = self.fixture(); before = p.stat()
            for _ in range(2):
                report = subject.check(root)
                self.assertTrue(report['writer_lock_reacquired'])
                self.assertFalse(report['continuous_exclusion_verified'])
                self.assertEqual(report['lock_inode'], before.st_ino)
                self.assertEqual(p.read_bytes(), b'preserved')
                self.assertEqual(p.stat().st_mtime_ns, before.st_mtime_ns)
            with p.open('r+b') as held:
                fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
                with self.assertRaisesRegex(ValueError, subject.ERROR): subject.check(root)
            self.assertTrue(subject.check(root)['writer_lock_reacquired'])

    def test_missing_link_fifo_permissions_and_hardlink_refused(self):
        for mode in ('missing','symlink','fifo','mode','root-mode','hardlink'):
            root,p = self.fixture()
            if mode in ('missing','symlink','fifo'): p.unlink()
            if mode=='symlink': p.symlink_to(root/'other')
            if mode=='fifo': os.mkfifo(p,0o600)
            if mode=='mode': p.chmod(0o644)
            if mode=='root-mode': root.chmod(0o755)
            if mode=='hardlink': os.link(p,root/'other')
            with self.assertRaisesRegex(ValueError,subject.ERROR): subject.check(root)
            if mode=='missing': self.assertFalse(p.exists())

    def test_replacement_and_interrupt_release_original_fd(self):
        for mode in ('replace','interrupt','unlock-error'):
            root,p=self.fixture(); calls=[]
            def hook(fd, op):
                calls.append(op)
                if op == fcntl.LOCK_UN and mode=='unlock-error': raise OSError('secret')
                fcntl.flock(fd, op)
                if op != fcntl.LOCK_UN:
                    if mode=='interrupt': raise KeyboardInterrupt()
                    if mode=='replace':
                        p.rename(root/'original'); p.write_bytes(b'new'); p.chmod(0o600)
            with self.assertRaises(KeyboardInterrupt if mode=='interrupt' else ValueError):
                subject._check(root,hook,lambda:1)
            original=root/'original' if mode=='replace' else p
            with original.open('r+b') as f: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
            self.assertEqual(calls.count(fcntl.LOCK_EX|fcntl.LOCK_NB),1)

    def test_clock_failure_releases_lock(self):
        root,p=self.fixture(); clock=iter([2,1])
        with self.assertRaisesRegex(ValueError,subject.ERROR):
            subject._check(root,fcntl.flock,lambda:next(clock))
        self.assertTrue(subject.check(root)['writer_lock_reacquired'])

if __name__=='__main__': unittest.main()
