import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import fresh_chain_home as m


class FreshHome(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.root.chmod(0o700)
        self.home = self.root / 'fee0'
        self.files = {k: ('synthetic-' + k).encode() for k in m.FILES}

    def run_publish(self):
        return m.publish(self.home, guard=b'guard', files=self.files)

    def test_exact_bytes_modes_and_separate_profiles(self):
        for fee in (0, 25):
            self.home = self.root / f'fee{fee}'
            report = self.run_publish()
            self.assertFalse(report['semantic_validation_verified'])
            self.assertFalse(report['service_started'])
            for p in (self.home, self.home/'config', self.home/'data'):
                self.assertEqual(p.stat().st_mode & 0o777, 0o700)
            for name, raw in self.files.items():
                p = self.home / ('data' if name == 'priv_validator_state.json' else 'config') / name
                self.assertEqual(p.read_bytes(), raw)
                self.assertEqual(p.stat().st_mode & 0o777, 0o600)
            self.assertEqual((self.home/'guard.dev.json').read_bytes(), b'guard')
            self.assertFalse((self.home/'writer.dev.lock').exists())

    def test_existing_and_symlink_never_replaced(self):
        self.run_publish()
        before = {str(p): p.read_bytes() for p in self.home.rglob('*') if p.is_file()}
        with self.assertRaises(FileExistsError): self.run_publish()
        self.assertEqual(before, {str(p): p.read_bytes() for p in self.home.rglob('*') if p.is_file()})
        link = self.root/'link'
        link.symlink_to(self.home, target_is_directory=True)
        self.home = link
        with self.assertRaises(FileExistsError): self.run_publish()

    def test_validation_before_effects(self):
        for bad in (b'', b'x' * ((1 << 20) + 1), 'not bytes'):
            with self.assertRaises(m.HomeError):
                m.publish(self.home, guard=bad, files=self.files)
            self.assertFalse(self.home.exists())
        self.root.chmod(0o755)
        with self.assertRaises(m.HomeError): self.run_publish()
        self.assertFalse(self.home.exists())

    def test_guard_fsync_failure_preserves_partial_home(self):
        original = os.fsync
        calls = []
        def fsync(fd):
            calls.append(fd)
            if len(calls) == 3:
                self.assertTrue((self.home/'guard.dev.json').is_file())
                self.assertFalse((self.home/'config').exists())
                raise OSError('injected guard directory fsync')
            return original(fd)
        with patch.object(m.os, 'fsync', fsync):
            with self.assertRaises(OSError): self.run_publish()
        self.assertEqual((self.home/'guard.dev.json').read_bytes(), b'guard')
        self.assertFalse((self.home/'data').exists())
        with self.assertRaises(FileExistsError): self.run_publish()

if __name__ == '__main__': unittest.main()
