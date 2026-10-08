import hashlib
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import fetch_process as f

class FetchProcessTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ['PAPERCLIP_RUN_SCRATCH_DIR'])
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.exe = self.root / 'fetch'
        self.raw = b'x' * (1024 * 1024)
    def script(self, body):
        if self.exe.exists(): self.exe.unlink()
        self.exe.write_text('#!' + sys.executable + '\nimport os,sys,time,hashlib\n' + body)
        self.exe.chmod(0o500)
    def call(self, **kw):
        args = dict(executable=self.exe,address='127.0.0.1:26657',pin='a'*64,
                    profile=self.root/'profile',acknowledge=True,raw=self.raw,timeout=2)
        args.update(kw)
        return f.fetch_captured(**args)
    def test_exact_input_argv_environment_and_raw_output(self):
        self.script('raw=sys.stdin.buffer.read()\n'
          'assert sys.argv[1:4]==["fetch-captured","--chain-rpc","127.0.0.1:26657"]\n'
          'assert sys.argv[4:6]==["--capture-sha256",hashlib.sha256(raw).hexdigest()]\n'
          'assert sys.argv[6:8]==["--runtime-pin","a"*64]\n'
          f'assert sys.argv[8:]==["--local-demo-profile",{str(self.root/"profile")!r},"--acknowledge-unproven-space"]\n'
          'assert not any(k.startswith(("PAPERCLIP_","DYLD_")) for k in os.environ)\n'
          'os.write(1,b\' { "raw": true } \\n\')\n')
        self.assertEqual(self.call(), b' { "raw": true } \n')
    def test_invalid_inputs_and_initial_stop_spawn_zero(self):
        with patch.object(f.subprocess, 'Popen', side_effect=AssertionError('spawn')):
            for kw in [dict(raw=b''),dict(timeout=True),dict(timeout=float('nan')),
                       dict(timeout=61),dict(acknowledge=False),dict(pin='A'*64),
                       dict(profile='/x/../y'),dict(executable='relative'),
                       *[dict(address=x) for x in ['localhost:26657','0.0.0.0:26657','127.0.0.1:80','127.0.0.1:026657']],
                       dict(stopped=lambda:True)]:
                with self.assertRaises(f.FetchFailure): self.call(**kw)
    def test_partial_nonzero_stderr_empty_and_flood_rejected(self):
        for tail in ['sys.exit(2)','os.write(2,b"not exposed")']:
            self.script('sys.stdin.buffer.read()\nos.write(1,b"partial")\n'+tail+'\n')
            with self.assertRaises(f.FetchFailure) as ctx:self.call()
            self.assertEqual(ctx.exception.partial_raw,b'partial')
            self.assertEqual(str(ctx.exception),'FETCH_REJECTED')
        self.script('sys.stdin.buffer.read()\n')
        with self.assertRaisesRegex(f.FetchFailure,'FETCH_REJECTED'):self.call()
        for stream in (1,2):
            self.script(f'sys.stdin.buffer.read()\nos.write({stream},b"x"*10000)\ntime.sleep(10)\n')
            with patch.object(f,'MAX_RPC',4096):
                with self.assertRaisesRegex(f.FetchFailure,'FETCH_OUTPUT_LIMIT') as ctx:self.call()
            self.assertLessEqual(len(ctx.exception.partial_raw),4096)
    def test_closed_pipes_without_exit_and_inherited_pipes_timeout(self):
        for body in ['sys.stdin.buffer.read()\nos.close(1)\nos.close(2)\ntime.sleep(10)\n',
                     'sys.stdin.buffer.read()\npid=os.fork()\nif pid==0: time.sleep(10); os._exit(0)\nos.write(1,b"partial")\n']:
            self.script(body)
            started=time.monotonic()
            with self.assertRaisesRegex(f.FetchFailure,'FETCH_TIMEOUT'):
                self.call(timeout=.5)
            self.assertLess(time.monotonic()-started,2)
    def test_timeout_stop_interrupt_reap(self):
        pidfile=self.root/'pid'
        for mode in ['timeout','stop','interrupt']:
            self.script(f'open({str(pidfile)!r},"w").write(str(os.getpid()))\n'
                        'sys.stdin.buffer.read()\nos.write(1,b"partial")\ntime.sleep(10)\n')
            def stop():
                if not pidfile.exists():return False
                if mode=='interrupt':raise KeyboardInterrupt()
                return mode=='stop'
            started=time.monotonic()
            with self.assertRaises((f.FetchFailure,KeyboardInterrupt)):
                self.call(timeout=.5,stopped=stop)
            self.assertLess(time.monotonic()-started,2)
            pid=int(pidfile.read_text())
            with self.assertRaises(ProcessLookupError):os.kill(pid,0)
            pidfile.unlink()

if __name__=='__main__':unittest.main()
