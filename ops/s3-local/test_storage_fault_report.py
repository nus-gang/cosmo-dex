import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import storage_fault_report as m


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.command = b'exact command bytes'
        self.sha = hashlib.sha256(self.command).hexdigest()
        self.first = dict(schema='s3-local-storage-fault/2', phase='reserved',
            command_sha256=self.sha, command_base64=base64.b64encode(self.command).decode(),
            point='before_wal', occurrence=1, visits=0, matching_visits=0,
            injected=False, durable_ack=False, DEV='NOT_RUN')
        self.last = dict(self.first, phase='scope_returned', visits=4, matching_visits=1, injected=True)

    def raw(self, *records):
        return b''.join(json.dumps(r).encode()+b'\n' for r in records)

    def test_complete_does_not_claim_seal_success_or_durability(self):
        for phase in ('scope_returned', 'scope_error', 'panic'):
            for injected in (False, True):
                last = dict(self.last, phase=phase, injected=injected, matching_visits=int(injected))
                r = m.inspect_bytes(self.raw(self.first, last), self.sha)
                self.assertEqual(r['observation'], 'RECORDED_INJECTION' if injected else 'RECORDED_NOT_REACHED')
                for k in ('seal_success_verified', 'fsync_verified', 'replay_verified', 'authenticity_verified', 'durable_ack'):
                    self.assertIs(r[k], False)

    def test_reserved_and_torn_final_are_unknown(self):
        for tail in (b'', b'{"phase":', json.dumps(self.last).encode()):
            r = m.inspect_bytes(self.raw(self.first)+tail, self.sha)
            self.assertEqual(r['observation'], 'UNKNOWN')
            self.assertIsNone(r['final_phase'])
        for raw in (b'', b'{', self.raw(self.first, self.last)+b'x', self.raw(self.first, self.last, self.last)):
            with self.assertRaisesRegex(ValueError, m.ERROR): m.inspect_bytes(raw, self.sha)

    def test_hash_schema_counters_and_duplicate_fields_reject(self):
        for changes in ({'command_base64':'YQ=='}, {'point':'wal_sync'}, {'occurrence':2},
                        {'visits':True}, {'matching_visits':2}, {'injected':False},
                        {'durable_ack':0}, {'DEV':'PASS'}, {'phase':'reserved'}, {'extra':1}):
            with self.assertRaisesRegex(ValueError, m.ERROR):
                m.inspect_bytes(self.raw(self.first, dict(self.last, **changes)), self.sha)
        for raw in (self.raw(self.first).replace(b'"visits": 0', b'"visits": 0, "visits": 0'), b'x'*(m.CAP+1)):
            with self.assertRaises(ValueError): m.inspect_bytes(raw, self.sha)
        with self.assertRaises(ValueError): m.inspect_bytes(self.raw(self.first), '0'*64)

    def test_errno_v3_binds_mode_and_rejects_mixed_or_unbound_records(self):
        for mode in ('ENOSPC', 'EDQUOT', 'EIO'):
            command = dict(io_fault=mode, point='before_wal', occurrence='1')
            data = json.dumps(command, sort_keys=True, separators=(',', ':')).encode()
            sha = hashlib.sha256(data).hexdigest()
            first = dict(self.first, schema='s3-local-storage-fault/3', io_fault=mode,
                         command_base64=base64.b64encode(data).decode(), command_sha256=sha)
            last = dict(first, phase='scope_error', visits=1, matching_visits=1, injected=True)
            self.assertEqual(m.inspect_bytes(self.raw(first, last), sha)['io_fault'], mode)
            self.assertEqual(m.inspect_bytes(self.raw(first), sha)['observation'], 'UNKNOWN')
            for changes in ({'io_fault': 'GENERIC'}, {'io_fault': 'OTHER'},
                            {'point': 'wal_sync'}, {'occurrence': 2},
                            {'schema': 's3-local-storage-fault/2'}):
                with self.assertRaises(ValueError):
                    m.inspect_bytes(self.raw(dict(first, **changes)), sha)
            missing = dict(first); del missing['io_fault']
            with self.assertRaises(ValueError): m.inspect_bytes(self.raw(missing), sha)
            # Matching report fields cannot override the selector hashed in command bytes.
            wrong = dict(first, io_fault='EIO' if mode != 'EIO' else 'EDQUOT')
            with self.assertRaises(ValueError): m.inspect_bytes(self.raw(wrong), sha)
            with self.assertRaises(ValueError): m.inspect_bytes(self.raw(self.first, last), self.sha)
            duplicate = data[:-1] + b',"io_fault":"EIO"}'
            bad = dict(first, command_base64=base64.b64encode(duplicate).decode(),
                       command_sha256=hashlib.sha256(duplicate).hexdigest())
            with self.assertRaises(ValueError): m.inspect_bytes(self.raw(bad), bad['command_sha256'])

    def test_readonly_filesystem_and_unsafe_objects(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve(); root.chmod(0o700)
            f = root/'storage-fault.jsonl'; raw=self.raw(self.first,self.last)
            f.write_bytes(raw); f.chmod(0o600)
            before=f.stat()
            self.assertEqual(m.inspect(root,self.sha)['observation'],'RECORDED_INJECTION')
            self.assertEqual(f.read_bytes(),raw)
            self.assertEqual((before.st_ino,before.st_mtime_ns),(f.stat().st_ino,f.stat().st_mtime_ns))
            f.chmod(0o644)
            with self.assertRaises(ValueError): m.inspect(root,self.sha)
            f.chmod(0o600); os.link(f,root/'alias')
            with self.assertRaises(ValueError): m.inspect(root,self.sha)
            (root/'alias').unlink(); f.unlink(); os.mkfifo(f,0o600)
            with self.assertRaises(ValueError): m.inspect(root,self.sha)
            f.unlink(); f.symlink_to(root/'missing')
            with self.assertRaises(ValueError): m.inspect(root,self.sha)

    def test_path_change_and_cli_open_stdin_denial(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve(); root.chmod(0o700)
            f=root/'storage-fault.jsonl'; f.write_bytes(self.raw(self.first)); f.chmod(0o600)
            read=os.read
            def changed(fd,n):
                data=read(fd,n)
                if data:
                    f.rename(root/'saved'); f.write_bytes(data); f.chmod(0o600)
                return data
            with patch.object(m.os,'read',changed):
                with self.assertRaises(ValueError): m.inspect(root,self.sha)
            self.assertTrue((root/'saved').exists())
        child=subprocess.Popen([sys.executable,m.__file__,'inspect'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            child.wait(timeout=2)
            self.assertEqual(child.returncode,2)
            self.assertEqual(child.stdout.read(),b'')
            self.assertEqual(child.stderr.read(),(m.ERROR+'\n').encode())
        finally:
            if child.poll() is None: child.kill(); child.wait()
            child.stdin.close(); child.stdout.close(); child.stderr.close()


if __name__ == '__main__': unittest.main()
