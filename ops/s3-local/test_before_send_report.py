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
import before_send_report as m

class BeforeSendReportTests(unittest.TestCase):
    def setUp(self):
        self.boundary=dict(schema='sre-before-send-boundary/1',boundary='F05',state='SUBMISSION_UNKNOWN',
            tx_hash='a'*64,stored_intent_sha256='b'*64,broadcast_count='1',raw_len='123',
            transport_called=False,crash_verified=False,reusable_permit=False)
        self.first,self.sha=self.row(self.boundary)

    def row(self,b):
        data=json.dumps(b,sort_keys=True,separators=(',',':')).encode()
        sha=hashlib.sha256(data).hexdigest()
        return dict(schema='s3-local-before-send/1',phase='reserved',boundary_sha256=sha,
            boundary_base64=base64.b64encode(data).decode(),transport_called=False,
            crash_verified=False,command_success_verified=False,durable_ack=False,DEV='NOT_RUN'),sha

    def raw(self,*rows):
        return b''.join(json.dumps(r).encode()+b'\n' for r in rows)

    def test_reservation_partial_and_return_states(self):
        for tail in (b'',b'{',b'{"phase":"panic"}'):
            self.assertEqual(m.inspect_bytes(self.raw(self.first)+tail,self.sha)['observation'],'UNKNOWN')
        for phase in ('boundary_returned','boundary_error','panic'):
            r=m.inspect_bytes(self.raw(self.first,dict(self.first,phase=phase)),self.sha)
            self.assertEqual(r['final_phase'],phase)
            for k in ('crash_verified','transport_verified','command_success_verified','authenticity_verified','fsync_verified','replay_verified','reusable_permit','durable_ack'):
                self.assertIs(r[k],False)

    def test_boundary_identity_and_caps(self):
        for changes in ({'state':'PREPARED'},{'boundary':'F06'},{'broadcast_count':'0'},
                        {'broadcast_count':'4'},{'broadcast_count':True},{'raw_len':'01'},
                        {'raw_len':'139265'},{'raw_len':1},{'tx_hash':'A'*64},
                        {'stored_intent_sha256':'x'},{'transport_called':0},{'reusable_permit':True},
                        {'extra':'secret'}):
            row,sha=self.row(dict(self.boundary,**changes))
            with self.assertRaisesRegex(ValueError,m.ERROR):m.inspect_bytes(self.raw(row),sha)
        for count in ('1','2','3'):
            row,sha=self.row(dict(self.boundary,broadcast_count=count,raw_len='139264'))
            m.inspect_bytes(self.raw(row),sha)

    def test_forged_mixed_duplicate_and_partial_records(self):
        for changes in ({'phase':'panic'},{'schema':'other'},{'boundary_sha256':'0'*64},
                        {'boundary_base64':'!'},{'durable_ack':0},{'crash_verified':True},{'extra':0}):
            with self.assertRaises(ValueError):m.inspect_bytes(self.raw(dict(self.first,**changes)),self.sha)
        duplicate=self.raw(self.first).replace(b'"phase": "reserved"',b'"phase":"reserved","phase":"reserved"')
        for raw in (b'',b'x'*(m.CAP+1),duplicate,self.raw(self.first)*2,self.raw(self.first)*3,
                    self.raw(self.first,dict(self.first,phase='panic'))+b'x'):
            with self.assertRaises(ValueError):m.inspect_bytes(raw,self.sha)
        row,sha=self.row(dict(self.boundary,raw_len='124'))
        with self.assertRaises(ValueError):m.inspect_bytes(self.raw(self.first,dict(row,phase='panic')),self.sha)
        data=b'{"schema":"x","schema":"y"}';sha=hashlib.sha256(data).hexdigest()
        with self.assertRaises(ValueError):m.inspect_bytes(self.raw(dict(self.first,boundary_sha256=sha,boundary_base64=base64.b64encode(data).decode())),sha)

    def test_readonly_and_unsafe_file_rejection(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();root.chmod(0o700)
            f=root/'before-send.jsonl';raw=self.raw(self.first);f.write_bytes(raw);f.chmod(0o600)
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
            f=root/'before-send.jsonl';f.write_bytes(self.raw(self.first));f.chmod(0o600)
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
