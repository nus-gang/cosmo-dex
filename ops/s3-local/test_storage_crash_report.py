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
import storage_crash_report as m


class CrashReportTests(unittest.TestCase):
    def setUp(self):
        self.command = dict(effect='IMMEDIATE_EXIT', exit_code=86, point='before_wal', occurrence='2')
        data = json.dumps(self.command, sort_keys=True, separators=(',', ':')).encode()
        self.sha = hashlib.sha256(data).hexdigest()
        self.first = dict(schema='s3-local-storage-crash/1', phase='reserved', command_sha256=self.sha,
            command_base64=base64.b64encode(data).decode(), point='before_wal', occurrence=2,
            visits=0, matching_visits=0, injected=False, durable_ack=False, DEV='NOT_RUN',
            effect='IMMEDIATE_EXIT', exit_code=86, crash_verified=False)

    def raw(self, *rows):
        return b''.join(json.dumps(r).encode()+b'\n' for r in rows)

    def test_reservation_and_partial_final_remain_unknown(self):
        for tail in (b'', b'{', b'{"phase":"scope_returned"}'):
            r=m.inspect_bytes(self.raw(self.first)+tail,self.sha)
            self.assertEqual(r['observation'],'UNKNOWN')
            self.assertIsNone(r['final_phase'])
            self.assertFalse(r['crash_verified'])

    def test_return_error_panic_are_not_success_or_crash(self):
        for phase in ('scope_returned','scope_error','panic'):
            last=dict(self.first,phase=phase,visits=4,matching_visits=1)
            r=m.inspect_bytes(self.raw(self.first,last),self.sha)
            self.assertEqual(r['observation'],'RECORDED_NOT_REACHED')
            for k in ('crash_verified','command_success_verified','authenticity_verified','fsync_verified','replay_verified','durable_ack'):
                self.assertIs(r[k],False)

    def test_inconsistent_forged_or_io_reports_reject(self):
        for changes in ({'schema':'s3-local-storage-fault/2'},{'exit_code':87},{'crash_verified':True},
                        {'injected':True},{'matching_visits':2,'visits':2},{'visits':True},
                        {'occurrence':1},{'point':'before_response'},{'effect':'IO_ERROR'},
                        {'command_sha256':'0'*64},{'extra':0},{'durable_ack':0}):
            with self.assertRaisesRegex(ValueError,m.ERROR):
                m.inspect_bytes(self.raw(dict(self.first,**changes)),self.sha)
        duplicate=self.raw(self.first).replace(b'"visits": 0',b'"visits": 0,"visits": 0')
        for raw in (b'', b'x'*(m.CAP+1),duplicate,self.raw(self.first,self.first),self.raw(self.first)*3):
            with self.assertRaises(ValueError):m.inspect_bytes(raw,self.sha)
        for changes in ({'effect':'IO_ERROR'},{'exit_code':True},{'occurrence':2}):
            data=json.dumps(dict(self.command,**changes)).encode();sha=hashlib.sha256(data).hexdigest()
            row=dict(self.first,command_sha256=sha,command_base64=base64.b64encode(data).decode())
            with self.assertRaises(ValueError):m.inspect_bytes(self.raw(row),sha)

    def test_readonly_and_unsafe_file_rejection(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();root.chmod(0o700)
            f=root/'storage-crash.jsonl';raw=self.raw(self.first);f.write_bytes(raw);f.chmod(0o600)
            before=f.stat()
            self.assertEqual(m.inspect(root,self.sha)['observation'],'UNKNOWN')
            after=f.stat()
            self.assertEqual((before.st_ino,before.st_mtime_ns,before.st_ctime_ns),(after.st_ino,after.st_mtime_ns,after.st_ctime_ns))
            self.assertEqual(f.read_bytes(),raw)
            f.chmod(0o644)
            with self.assertRaises(ValueError):m.inspect(root,self.sha)
            f.chmod(0o600);os.link(f,root/'alias')
            with self.assertRaises(ValueError):m.inspect(root,self.sha)
            (root/'alias').unlink();f.unlink();os.mkfifo(f,0o600)
            with self.assertRaises(ValueError):m.inspect(root,self.sha)
            f.unlink();f.symlink_to(root/'missing')
            with self.assertRaises(ValueError):m.inspect(root,self.sha)

    def test_path_replacement_and_open_stdin_cli_reject(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();root.chmod(0o700)
            f=root/'storage-crash.jsonl';f.write_bytes(self.raw(self.first));f.chmod(0o600)
            read=os.read
            def change(fd,n):
                data=read(fd,n)
                if data:f.rename(root/'saved');f.write_bytes(data);f.chmod(0o600)
                return data
            with patch('storage_fault_report.os.read',change):
                with self.assertRaises(ValueError):m.inspect(root,self.sha)
        child=subprocess.Popen([sys.executable,m.__file__,'inspect'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            child.wait(timeout=2);self.assertEqual(child.returncode,2)
            self.assertEqual(child.stdout.read(),b'');self.assertEqual(child.stderr.read(),(m.ERROR+'\n').encode())
        finally:
            if child.poll() is None:child.kill();child.wait()
            child.stdin.close();child.stdout.close();child.stderr.close()


if __name__=='__main__':unittest.main()
