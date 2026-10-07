import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from response_loss import ResponseLoss
from fault_evidence import record


class FaultEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)
        self.body = b'private TX'
        self.calls = []
        self.fault = ResponseLoss(lambda *a: self.calls.append(a), destination=('127.0.0.1',8787),
            body_sha256=hashlib.sha256(self.body).hexdigest(), enable_local_demo=True, allow_unproven_host_space=True)
    def invoke(self):
        self.fault(('127.0.0.1',8787), b'POST /dev-local/v1/chain/broadcast HTTP/1.1\r\nAuthorization: Bearer secret\r\n\r\n'+self.body)
    def rows(self):
        return [json.loads(line) for line in (self.root/'response-loss.jsonl').read_bytes().splitlines()]
    def test_effect_error_preserved_and_no_secrets(self):
        def action():
            self.assertEqual(self.rows()[0]['phase'], 'reserved')
            self.invoke()
        with self.assertRaisesRegex(OSError,'RESPONSE_LOSS_INJECTED'):
            record(self.root,self.fault,action)
        self.assertEqual(len(self.calls),1)
        rows=self.rows()
        self.assertEqual(rows[1]['phase'],'interrupted_or_failed')
        self.assertTrue(rows[1]['report']['response_discarded'])
        self.assertEqual((self.root/'response-loss.jsonl').stat().st_mode & 0o777,0o600)
        self.assertNotIn('secret',repr(rows)); self.assertNotIn('private',repr(rows))
        with self.assertRaises(ValueError): record(self.root,self.fault,action)
        self.assertEqual(len(self.calls),1)
    def test_return_and_base_exceptions(self):
        for error in (None, KeyboardInterrupt(), SystemExit(), OSError('secret')):
            root=self.root/str(len(list(self.root.iterdir()))); root.mkdir(mode=0o700)
            def action():
                if error: raise error
                return 42
            if error:
                with self.assertRaises(type(error)): record(root,self.fault,action)
            else: self.assertEqual(record(root,self.fault,action),42)
            rows=[json.loads(x) for x in (root/'response-loss.jsonl').read_text().splitlines()]
            self.assertEqual(len(rows),2)
            self.assertFalse(rows[1]['report']['used'])
            self.assertEqual(rows[1]['phase'],'interrupted_or_failed' if error else 'returned')
    def test_reservation_and_fsync_failure_prevent_action(self):
        path=self.root/'response-loss.jsonl'
        for kind in ('existing','symlink','fifo','sync'):
            if kind=='existing': path.write_text('old')
            if kind=='symlink': path.symlink_to(self.root/'absent')
            if kind=='fifo': os.mkfifo(path)
            with patch('fault_evidence.os.fsync',side_effect=OSError('sync')):
                with self.assertRaises(OSError): record(self.root,self.fault,lambda:self.fail('action'))
            self.assertTrue(path.exists() or path.is_symlink())
            path.unlink()
        self.root.chmod(0o755)
        with self.assertRaisesRegex(ValueError,'FAULT_EVIDENCE_ROOT'):
            record(self.root,self.fault,lambda:self.fail('action'))
    def test_replacement_and_final_sync_failure_preserve_unknown(self):
        path=self.root/'response-loss.jsonl'
        def action():
            path.rename(self.root/'preserved'); path.write_text('replacement')
        with self.assertRaisesRegex(ValueError,'FAULT_EVIDENCE_FILE_CHANGED'):
            record(self.root,self.fault,action)
        self.assertEqual(path.read_text(),'replacement')
        self.assertEqual(len((self.root/'preserved').read_text().splitlines()),1)
        path.unlink()
        real=os.fsync; count=0
        def sync(fd):
            nonlocal count
            count+=1
            if count==3: raise OSError('final sync')
            real(fd)
        with patch('fault_evidence.os.fsync',side_effect=sync):
            with self.assertRaisesRegex(OSError,'final sync'): record(self.root,self.fault,lambda:42)
        raw=path.read_bytes()
        with self.assertRaises(FileExistsError): record(self.root,self.fault,lambda:self.fail('retry'))
        self.assertEqual(path.read_bytes(),raw)
