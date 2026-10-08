import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from process_check import validate_captured, EXPECTED

class ProcessCheckTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=os.environ.get('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.executable = self.root / 'fixture-validator'
    def script(self, body):
        self.executable.write_text('#!' + sys.executable + '\nimport os, sys, json, hashlib, time\n' + body)
        self.executable.chmod(0o700)
        return str(self.executable)
    def check(self, body, raw=b'test-only capture', timeout=2):
        return validate_captured(self.script(body), ['--test-only'], raw, timeout)
    def test_exact_large_input_argv_empty_environment_and_success_cleanup(self):
        body = ('raw=sys.stdin.buffer.read()\n'
                'assert sys.argv[1:3]==["validate-captured","--capture-sha256"]\n'
                'assert sys.argv[3]==hashlib.sha256(raw).hexdigest()\n'
                'assert sys.argv[4:]==["--test-only"]\n'
                'assert not any(k.startswith("PAPERCLIP_") or k.startswith("DYLD_") for k in os.environ)\n'
                f'print({json.dumps(json.dumps(EXPECTED))})\n')
        self.assertEqual(self.check(body, b'x' * (2 * 1024 * 1024)), EXPECTED)
    def test_no_read_timeout_and_output_flood_are_bounded(self):
        for body in ('time.sleep(10)\n', 'os.write(1,b"x"*16384)\ntime.sleep(10)\n',
                     'os.write(2,b"x"*16384)\ntime.sleep(10)\n'):
            started = time.monotonic()
            with self.assertRaises(ValueError): self.check(body, b'x'*1048576, .2)
            self.assertLess(time.monotonic()-started, 2)
    def test_rejection_truncation_wrong_types_duplicate_and_claimed_approval(self):
        for report in ('{}', '{', json.dumps(dict(EXPECTED, semantic_validation=1)),
                       json.dumps(dict(EXPECTED, approval_verified=True)),
                       '{"semantic_validation":true,"semantic_validation":false}'):
            with self.assertRaises(ValueError):
                self.check('sys.stdin.buffer.read()\nprint('+repr(report)+')\n')
        for tail in ('sys.exit(2)', 'sys.stderr.write("secret not relayed")'):
            with self.assertRaises(ValueError):
                self.check('sys.stdin.buffer.read()\nprint('+repr(json.dumps(EXPECTED))+')\n'+tail+'\n')
    def test_eof_without_exit_and_descendant_holding_pipe_are_killed(self):
        for body in ('os.close(0)\nos.close(1)\nos.close(2)\ntime.sleep(10)\n',
                     'pid=os.fork()\nif pid==0: time.sleep(10); os._exit(0)\n'
                     'sys.stdin.buffer.read()\nprint('+repr(json.dumps(EXPECTED))+')\n'):
            started=time.monotonic()
            with self.assertRaises(ValueError): self.check(body, timeout=.2)
            self.assertLess(time.monotonic()-started,2)
    def test_reaped_child_and_invalid_input_before_spawn(self):
        pidfile=self.root/'pid'
        exe=self.script('open('+repr(str(pidfile))+',"w").write(str(os.getpid()))\ntime.sleep(10)\n')
        with self.assertRaises(ValueError): validate_captured(exe,[],b'x',.2)
        pid=int(pidfile.read_text())
        with self.assertRaises(ProcessLookupError): os.kill(pid,0)
        pidfile.unlink()
        for raw,timeout in ((b'',1),(b'x',0),(b'x',61),(b'x',float('nan'))):
            with self.assertRaises(ValueError): validate_captured(exe,[],raw,timeout)
        self.assertFalse(pidfile.exists())

if __name__=='__main__': unittest.main()
