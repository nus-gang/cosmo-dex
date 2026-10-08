import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import pid_mailbox as pm

class MailboxTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.parent = Path(self.tmp.name).resolve()
        self.box = pm.Mailbox(self.parent / 'inventory')

    def test_cross_process_record_and_one_shot(self):
        program = ('import os,sys,pid_mailbox; '
                   'pid_mailbox.reporter(sys.argv[1])(2147483647); print(os.getpid())')
        result = subprocess.run([sys.executable,'-B','-c',program,str(self.box.root)],
                                capture_output=True, timeout=3, check=True)
        self.assertEqual(self.box.collect(), (int(result.stdout), 2147483647))
        with self.assertRaises(ValueError): self.box.collect()
        with self.assertRaises(ValueError): pm.Mailbox(self.box.root)
        self.assertEqual((self.box.root/'pids.json').stat().st_mode & 0o777, 0o600)

    def test_actual_ready_child_handoff_before_start_and_reap(self):
        import test_ready_worker
        from ready_worker import ready
        fixture = test_ready_worker.ReadyTest()
        fixture.setUp(); self.addCleanup(fixture.doCleanups)
        staged = fixture.worker('s.sendall(b"READY\\n")\ns.recv(16)\n')
        with ready(staged, [], lambda: 'same', 2,
                   on_spawn=pm.reporter(self.box.root)):
            pids = self.box.collect()
            self.assertEqual(pids, (os.getpid(), int(fixture.pidfile.read_text())))
        with self.assertRaises(ProcessLookupError): os.kill(pids[1], 0)

    def test_missing_partial_stale_and_noncanonical_fail_once(self):
        for raw in (None,b'{',b'x'*513):
            root = self.parent / ('case'+str(len(list(self.parent.iterdir()))))
            box = pm.Mailbox(root)
            if raw is not None: pm._write(root,'pids.json',raw)
            with self.assertRaises(ValueError): box.collect()
            with self.assertRaises(ValueError): box.collect()
        pm.reporter(self.box.root)(2147483647)
        path = self.box.root/'pids.json'
        saved = path.read_bytes()
        for mutate in ('nonce','worker_pid','extra','whitespace'):
            box = pm.Mailbox(self.parent/mutate)
            doc = json.loads(saved); doc['nonce'] = box.nonce
            if mutate == 'nonce': doc['nonce'] = '0'*64
            elif mutate == 'worker_pid': doc['worker_pid'] = True
            elif mutate == 'extra': doc['extra'] = 1
            raw = json.dumps(doc,sort_keys=True,separators=(',',':')).encode()
            if mutate == 'whitespace': raw += b'\n'
            pm._write(box.root,'pids.json',raw)
            with self.assertRaises(ValueError): box.collect()

    def test_links_permissions_fifo_and_callback_poison(self):
        challenge = self.box.root/'challenge'
        for kind in ('mode','symlink','hardlink','fifo'):
            box = pm.Mailbox(self.parent/kind)
            path = box.root/'challenge'
            if kind == 'mode': path.chmod(0o644)
            else:
                path.unlink()
                if kind == 'symlink': path.symlink_to(challenge)
                elif kind == 'hardlink': os.link(challenge,path)
                else: os.mkfifo(path,0o600)
            with self.assertRaises(ValueError): pm.reporter(box.root)
            if kind == 'hardlink': path.unlink()
        record = pm.reporter(self.box.root)
        with self.assertRaises(ValueError): record(True)
        with self.assertRaises(ValueError): record(2147483647)
        self.assertFalse((self.box.root/'pids.json').exists())

    def test_replacement_and_write_failure_preserve_evidence(self):
        record = pm.reporter(self.box.root)
        with patch.object(pm.os,'fsync',side_effect=OSError('private')):
            with self.assertRaisesRegex(ValueError,'^PID_HANDOFF_REJECTED$'): record(2147483647)
        self.assertTrue((self.box.root/'pids.json').exists())
        with self.assertRaises(ValueError): record(2147483647)
        old = self.parent/'old'
        self.box.root.rename(old)
        pm.Mailbox(self.box.root)
        with self.assertRaises(ValueError): self.box.collect()
        self.assertTrue((old/'pids.json').exists())

if __name__ == '__main__': unittest.main()
