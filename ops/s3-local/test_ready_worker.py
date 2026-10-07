import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from staged_worker import StagedWorker
from ready_worker import ready

class ReadyTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=os.environ.get('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.pidfile = self.root / 'pid'

    def worker(self, action):
        path = self.root / 'worker'
        raw = ('#!' + sys.executable + '\nimport os,socket,sys,time\n' +
               f'open({str(self.pidfile)!r},"w").write(str(os.getpid()))\n' +
               'assert not any(k.startswith(("PAPERCLIP_","DYLD_")) for k in os.environ)\n' +
               'assert sys.stdin.buffer.read()==b"capture"\n' +
               's=socket.socket(fileno=int(sys.argv[3]))\n' + action).encode()
        if path.exists(): path.chmod(0o700)
        path.write_bytes(raw)
        path.chmod(0o500)
        return StagedWorker(path,b'capture',hashlib.sha256(b'capture').hexdigest(),hashlib.sha256(raw).hexdigest())

    def reaped(self):
        if self.pidfile.exists():
            with self.assertRaises(ProcessLookupError): os.kill(int(self.pidfile.read_text()),0)

    def test_ready_reaudit_no_start_and_reap(self):
        calls=[]
        staged=self.worker('s.sendall(b"READY\\n")\nassert s.recv(16)==b""\n')
        with ready(staged,[],lambda: calls.append(1) or 'same',2) as report:
            self.assertEqual(len(calls),2)
            self.assertFalse(report['service_started'])
            self.assertFalse(report['reusable_permit'])
        self.reaped()

    def test_revoked_changed_interrupted_and_mutated_after_ready(self):
        for mode in ['revoked','changed','interrupt','mutated']:
            staged=self.worker('s.sendall(b"READY\\n")\ns.recv(16)\n')
            calls=[]
            def audit():
                calls.append(1)
                if len(calls)==2:
                    if mode=='revoked': raise ValueError('revoked')
                    if mode=='interrupt': raise KeyboardInterrupt()
                    if mode=='changed': return 'other'
                    staged.executable.chmod(0o700)
                    staged.executable.write_bytes(b'changed')
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)), ready(staged,[],audit,2):
                self.fail('yielded')
            self.assertEqual(len(calls),2)
            self.reaped()

    def test_protocol_exit_timeout_output_fail_closed(self):
        for action in ['s.sendall(b"READY\\nX")\ntime.sleep(2)\n',
                       'sys.exit(0)\n','time.sleep(2)\n',
                       'print("secret",flush=True)\ntime.sleep(2)\n']:
            staged=self.worker(action)
            with self.assertRaises(ValueError), ready(staged,[],lambda:'same',.2):
                self.fail('yielded')
            self.reaped()

    def test_initial_denial_and_changed_bytes_spawn_zero(self):
        staged=self.worker('time.sleep(2)\n')
        def deny(): raise ValueError('denied')
        with self.assertRaises(ValueError), ready(staged,[],deny): self.fail('yielded')
        self.assertFalse(self.pidfile.exists())
        staged.executable.chmod(0o700)
        staged.executable.write_bytes(b'changed')
        with self.assertRaises(ValueError), ready(staged,[],lambda:'same'): self.fail('yielded')
        self.assertFalse(self.pidfile.exists())

if __name__=='__main__': unittest.main()
