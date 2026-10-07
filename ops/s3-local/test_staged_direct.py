import base64
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from manifest import encode
from offline_check import DESCRIPTOR
import staged_direct as subject

class DirectStageTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.artifacts = self.root/'artifacts'
        (self.artifacts/'bin').mkdir(parents=True)
        self.scratch = self.root/'scratch'
        self.scratch.mkdir(mode=0o700)
        self.original = self.artifacts/subject.HELPER
        self.original.write_bytes(b'SYNTHETIC_NOT_EXECUTED')
        self.digest = hashlib.sha256(self.original.read_bytes()).hexdigest()
        self.capture = self.make_capture({subject.HELPER:self.digest})
    def make_capture(self, inventory):
        descriptor = encode({'implementation_settings':{'artifacts_sha256_json':encode(inventory).decode()}})
        return encode({'files':{DESCRIPTOR:base64.b64encode(descriptor).decode()}})
    def stage(self, capture=None):
        return subject.stage_snapshot(self.capture if capture is None else capture, self.artifacts, self.scratch)
    def test_snapshot_replacement_permissions_and_cleanup(self):
        with self.stage() as staged:
            self.original.write_bytes(b'REPLACED')
            self.assertEqual(staged.executable.read_bytes(), b'SYNTHETIC_NOT_EXECUTED')
            self.assertEqual(staged.executable.stat().st_mode & 0o777, 0o500)
            self.assertEqual(staged.executable.parent.stat().st_mode & 0o777, 0o700)
            self.assertEqual(staged.capture_sha256, hashlib.sha256(self.capture).hexdigest())
            staged.recheck()
        self.assertFalse(staged.executable.exists())
        self.assertEqual(self.original.read_bytes(), b'REPLACED')
    def test_missing_digest_mutation_and_links_rejected(self):
        for inventory in ({}, {subject.HELPER:'A'*64}, {subject.HELPER:'0'*64}):
            with self.assertRaises(ValueError), self.stage(self.make_capture(inventory)): pass
        self.original.rename(self.artifacts/'saved')
        self.original.symlink_to(self.artifacts/'saved')
        with self.assertRaises(OSError), self.stage(): pass
        self.original.unlink()
        os.link(self.artifacts/'saved', self.original)
        with self.assertRaises(ValueError), self.stage(): pass
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_private_mutation_and_interrupt_cleanup(self):
        with self.assertRaisesRegex(ValueError, 'STAGED_DIRECT_BYTES_CHANGED'):
            with self.stage() as staged:
                staged.executable.chmod(0o600)
                staged.executable.write_bytes(b'CHANGED')
                staged.recheck()
        with self.assertRaises(KeyboardInterrupt):
            with self.stage(): raise KeyboardInterrupt()
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_write_failure_and_mutable_capture_rejected(self):
        with patch.object(subject.os, 'fsync', side_effect=OSError('injected')):
            with self.assertRaises(OSError), self.stage(): pass
        with self.assertRaises(ValueError), self.stage(bytearray(self.capture)): pass
        self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
