"""Real synthetic subprocess IPC, no Go helper or Rust service execution."""
import base64
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from manifest import encode
from offline_check import DESCRIPTOR
from staged_direct import HELPER
from staged_worker import StagedWorker
from direct_worker import ready_with_direct, run_with_direct

class DirectWorkerTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(dir=os.environ.get('PAPERCLIP_RUN_SCRATCH_DIR'))
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name).resolve()
        self.artifacts = self.root/'artifacts'
        (self.artifacts/'bin').mkdir(parents=True)
        self.original = self.artifacts/HELPER
        self.original.write_bytes(b'SYNTHETIC_HELPER_NOT_EXECUTED')
        self.scratch = self.root/'scratch'
        self.scratch.mkdir(mode=0o700)
        self.pid = self.root/'pid'
        self.path = self.root/'helper-path'
        self.started = self.root/'started'
        digest = hashlib.sha256(self.original.read_bytes()).hexdigest()
        descriptor = encode({'implementation_settings':{'artifacts_sha256_json':encode({HELPER:digest}).decode()}})
        self.capture = encode({'files':{DESCRIPTOR:base64.b64encode(descriptor).decode()}})

    def worker(self):
        raw = ('#!'+sys.executable+'\nimport os,sys,socket,hashlib\nfrom pathlib import Path\n'+
               f'Path({str(self.pid)!r}).write_text(str(os.getpid()))\n'+
               f'assert sys.stdin.buffer.read()=={self.capture!r}\n'+
               'assert sys.argv[6]=="--direct-helper"\n'+
               'helper=Path(sys.argv[7])\nassert helper.is_absolute()\n'+
               'assert sys.argv[8]=="--direct-helper-sha256"\n'+
               'assert hashlib.sha256(helper.read_bytes()).hexdigest()==sys.argv[9]\n'+
               f'Path({str(self.path)!r}).write_text(str(helper))\n'+
               's=socket.socket(fileno=int(sys.argv[3]))\ns.sendall(b"READY\\n")\n'+
               'command=s.recv(16)\nif command:\n'+
               ' assert command==b"START\\n"\n assert s.recv(16)==b""\n'+
               ' assert helper.read_bytes()==b"SYNTHETIC_HELPER_NOT_EXECUTED"\n'+
               f' Path({str(self.started)!r}).write_text("IPC_ONLY")\n').encode()
        worker = self.root/'worker'
        worker.write_bytes(raw)
        worker.chmod(0o500)
        return StagedWorker(worker,self.capture,hashlib.sha256(self.capture).hexdigest(),hashlib.sha256(raw).hexdigest())

    def cleaned(self):
        self.assertEqual(list(self.scratch.iterdir()), [])
        if self.pid.exists():
            with self.assertRaises(ProcessLookupError): os.kill(int(self.pid.read_text()),0)
        if self.path.exists(): self.assertFalse(Path(self.path.read_text()).exists())

    def test_checked_entrypoints_use_helper_scope(self):
        from contextlib import contextmanager
        from unittest.mock import patch
        from staged_worker import checked_ready, checked_run
        staged = self.worker()
        @contextmanager
        def validated(*args):
            yield staged
        args = [self.root, self.artifacts, 'synthetic-pin', 's3-dev-local/1',
                True, 'synthetic-decision', {}, self.root, 'input.json', [], self.scratch]
        with patch('staged_worker.validated_stage', validated), \
             patch('approval_gate.inspect', return_value={'fixture':'same'}):
            with checked_ready(*args, timeout=2) as report:
                self.assertTrue(report['ready'])
                self.assertTrue(Path(self.path.read_text()).exists())
                self.assertFalse(self.started.exists())
            self.cleaned()
            result = checked_run(*args, stop=lambda:False, timeout=2, lifetime=1)
            self.assertEqual(result['worker_exit'], 0)
        self.assertEqual(self.started.read_text(), 'IPC_ONLY')
        self.cleaned()

    def test_ready_scope_preserves_helper_until_reap(self):
        with ready_with_direct(self.worker(),[],self.artifacts,self.scratch,lambda:'same',2) as report:
            self.assertFalse(report['service_started'])
            helper = Path(self.path.read_text())
            self.original.write_bytes(b'REPLACED')
            self.assertEqual(helper.read_bytes(), b'SYNTHETIC_HELPER_NOT_EXECUTED')
            self.assertFalse(self.started.exists())
        self.cleaned()

    def test_synthetic_start_and_helper_lifetime(self):
        report=run_with_direct(self.worker(),[],self.artifacts,self.scratch,lambda:'same',lambda:False,2,1)
        self.assertEqual(report['worker_exit'],0)
        self.assertEqual(self.started.read_text(),'IPC_ONLY')
        self.cleaned()

    def test_after_ready_mutation_revocation_interrupt_start_zero(self):
        staged=self.worker()
        for mode in ('mutation','revocation','interrupt'):
            def audit():
                if self.path.exists():
                    if mode=='revocation': return 'revoked'
                    if mode=='interrupt': raise KeyboardInterrupt()
                    helper=Path(self.path.read_text())
                    helper.chmod(0o600)
                    helper.write_bytes(b'CHANGED')
                return 'same'
            with self.assertRaises((ValueError,KeyboardInterrupt)):
                run_with_direct(staged,[],self.artifacts,self.scratch,audit,lambda:False,2,1)
            self.assertFalse(self.started.exists())
            self.cleaned()
            self.path.unlink()
            self.pid.unlink()

    def test_last_audit_mutation_prevents_start(self):
        calls=[]
        def audit():
            calls.append(1)
            if len(calls)==5:
                helper=Path(self.path.read_text())
                helper.chmod(0o600)
                helper.write_bytes(b'CHANGED_AT_START')
            return 'same'
        with self.assertRaisesRegex(ValueError,'STAGED_DIRECT_BYTES_CHANGED'):
            run_with_direct(self.worker(),[],self.artifacts,self.scratch,audit,lambda:False,2,1)
        self.assertEqual(len(calls),5)
        self.assertFalse(self.started.exists())
        self.cleaned()

    def test_override_and_missing_descriptor_spawn_zero(self):
        staged=self.worker()
        for argv in (['--direct-helper','/other'],['--direct-helper=/other'],['--direct-helper-sha256','0'*64],[None]):
            with self.assertRaisesRegex(ValueError,'DIRECT_HELPER_OVERRIDE'):
                with ready_with_direct(staged,argv,self.artifacts,self.scratch,lambda:'same',2): self.fail()
        self.original.write_bytes(b'CHANGED')
        with self.assertRaisesRegex(ValueError,'DIRECT_BYTES_CHANGED'):
            with ready_with_direct(staged,[],self.artifacts,self.scratch,lambda:'same',2): self.fail()
        self.assertFalse(self.pid.exists())
        self.cleaned()

if __name__=='__main__': unittest.main()
