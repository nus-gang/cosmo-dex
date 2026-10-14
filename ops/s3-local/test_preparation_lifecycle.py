import hashlib
import io
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import tempfile
import unittest
from contextlib import contextmanager, redirect_stderr
from unittest.mock import Mock, patch

import key_preparation as k
import key_preparation_cli as child
import preparation_status as m
import restart_session_cli as cli
from static_web import StaticWeb
from web_pid_mailbox import WebMailbox, reporter
from test_registration_render import fixture
from registration_render import key_packets
from test_workspace_reentry import synthetic
import workspace_reentry
from pid_mailbox import _write


class PreparationTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.count=0
        self.proxy=k.KeyPreparation(origin='http://127.0.0.1:5173',worker_port=18080,
            static=StaticWeb(origin='http://127.0.0.1:5173',html=b'approved html',javascript=b'js'))

    def trace(self):
        self.count+=1;box=WebMailbox(self.root/str(self.count));reporter(box.root)()
        return box,m.Trace(box.root)

    def ready(self,trace):
        for phase in m.PHASES[1:6]:
            trace.emit(phase,**({'html_sha256':hashlib.sha256(b'approved html').hexdigest()}
                              if phase=='http_listening' else {}))

    def test_cancellation_and_rejection_stages_never_bind_before_audits(self):
        for which in (1,2):
            for action in ('stop','approval_error'):
                with self.subTest(which=which,action=action):
                    box,trace=self.trace();stop=[False];count=[0];socket=Mock()
                    def audit(*args):
                        count[0]+=1
                        if count[0]==which:
                            if action=='stop':stop[0]=True
                            else:raise ValueError('secret text must not be recorded')
                        return {}
                    with patch.object(k.approval_gate,'inspect',side_effect=audit),patch.object(k,'prepare',return_value=self.proxy):
                        with self.assertRaises(ValueError):
                            k.run(None,None,None,None,None,{},lambda:stop[0],None,socket_factory=socket,trace=trace)
                    socket.assert_not_called();rows=m.events(box)
                    self.assertEqual(rows[-1]['cause'],'operator_stop' if action=='stop' else 'approval')
                    self.assertEqual(rows[-1]['failed_phase'],'audit_first' if which==1 else 'audit_final')
                    self.assertNotIn('secret',repr(rows));self.assertEqual(box.collect(),(os.getpid(),))

    def test_bind_failure_and_after_ready_stop_have_distinct_evidence(self):
        for fail in (True,False):
            box,trace=self.trace();listener=Mock();stop=[False]
            if fail:listener.bind.side_effect=OSError('secret host path')
            def serve(*args,**kw):
                self.assertEqual(m.events(box)[-1]['phase'],'http_listening')
                stop[0]=True;return {'listener_closed':True}
            with patch.object(k.approval_gate,'inspect',return_value={}),patch.object(k,'prepare',return_value=self.proxy),patch.object(k,'serve_listener',side_effect=serve):
                call=lambda:k.run(None,None,None,None,None,{},lambda:stop[0],None,socket_factory=lambda *a:listener,trace=trace)
                if fail:
                    with self.assertRaises(OSError):call()
                    self.assertEqual(m.events(box)[-1]['cause'],'port')
                else:
                    call();self.assertEqual(m.events(box)[-1]['phase'],'closed')
            listener.close.assert_called_once()

    def test_real_subprocess_sigterm_and_approval_failure_before_listener(self):
        spec,_=fixture();spec['control_root']=str(self.root)
        for index,mode in enumerate(('sigterm-first','sigterm-final','approval-first','approval-final')):
            spec['control_root']=str(self.root/str(index));packet=key_packets(spec)[0]
            kind,a,_=workspace_reentry.command_info(packet)
            a.pid_mailbox.parent.mkdir(mode=0o700,parents=True);box=WebMailbox(a.pid_mailbox)
            argv=shlex.split(packet['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command'])[4:]
            script='''
import sys,json,os,signal
from contextlib import nullcontext
from unittest.mock import patch
import key_preparation_cli as c,key_preparation as k
calls=0
def audit(*args):
 global calls
 calls+=1
 if calls==(1 if sys.argv[1].endswith('first') else 2):
  if sys.argv[1].startswith('sigterm'):os.kill(os.getpid(),signal.SIGTERM)
  else:raise ValueError('DO_NOT_LOG_SECRET')
 return {}
with patch.object(c.approval_gate,'private_transport',return_value=nullcontext()),patch.object(k.approval_gate,'inspect',side_effect=audit),patch.object(k,'prepare',return_value=object()):
 sys.exit(c.main(json.loads(sys.argv[2])))
'''
            result=subprocess.run([sys.executable,'-B','-c',script,mode,json.dumps(argv)],
                cwd=Path(__file__).parent,capture_output=True,timeout=5)
            self.assertEqual(result.returncode,2,result.stderr)
            self.assertEqual(result.stderr,b'LOCAL_KEY_PREPARATION_REJECTED\n')
            rows=m.events(box);self.assertEqual(rows[-1]['cause'],'operator_stop' if mode.startswith('sigterm') else 'approval')
            self.assertNotEqual(rows[0]['pid'],os.getpid());self.assertEqual(len(box.collect()),1)

    def test_readiness_waits_for_http_and_has_independent_deadline(self):
        box,trace=self.trace();now=[0.];probe=Mock();published=[False]
        def sleep(delta):
            now[0]+=delta
            if now[0]>5.383164 and not published[0]:self.ready(trace);published[0]=True
        report=m.wait_ready(box,'http://127.0.0.1:5173',lambda:False,60,
            clock=lambda:now[0],sleep=sleep,probe=probe)
        self.assertGreater(report['preparation_seconds'],5.383164);probe.assert_called_once()
        self.assertFalse(report['approval_verified'])
        for case in ('timeout','stop','backwards','http_error'):
            b,t=self.trace();n=[0.]
            if case=='http_error':self.ready(t)
            def pause(delta):n[0]+=delta if case!='backwards' else -1
            bad=Mock(side_effect=OSError())
            with self.subTest(case=case),self.assertRaises((ValueError,OSError)):
                m.wait_ready(b,'http://127.0.0.1:5173',lambda:case=='stop',1,
                    clock=lambda:n[0],sleep=pause,probe=bad)
            if case!='http_error':bad.assert_not_called()
            self.assertTrue(b.root.exists())

    def test_partial_link_nonce_and_terminal_trace_are_never_ready(self):
        for case in ('partial','link','nonce','pending_name','closed','failed'):
            box,trace=self.trace();self.ready(trace)
            path=trace.root/'05.json'
            if case=='partial':path.write_text('{')
            if case=='link':path.rename(trace.root/'saved');path.symlink_to('saved')
            if case=='nonce':(box.root/'challenge').write_text('e'*64)
            if case=='pending_name':path.rename(trace.root/'05.partial')
            if case=='closed':trace.emit('closed')
            if case=='failed':trace.emit('failed',cause='operator_stop')
            probe=Mock()
            with self.subTest(case=case),self.assertRaises((ValueError,OSError)):
                m.wait_ready(box,'http://127.0.0.1:5173',lambda:False,probe=probe)
            probe.assert_not_called()

    def test_http_probe_uses_exact_host_bytes_no_redirect_and_closes(self):
        for status,body,length in ((200,b'approved html','13'),(302,b'approved html','13'),(200,b'wrong','5')):
            conn=Mock();conn.recv.return_value=(f'HTTP/1.1 {status} Response\r\nContent-Length: {length}\r\n\r\n').encode()+body
            call=lambda:m.http_ready('http://localhost:5173',hashlib.sha256(b'approved html').hexdigest(),1,connect=lambda *a,**kw:conn)
            if status==200 and body==b'approved html':call()
            else:
                with self.assertRaises(ValueError):call()
            conn.sendall.assert_called_once_with(b'GET / HTTP/1.1\r\nHost: localhost:5173\r\nConnection: close\r\n\r\n')
            conn.close.assert_called_once()
        conn=Mock();conn.recv.return_value=b'H';now=[0.]
        def clock():now[0]+=0.1;return now[0]
        with self.assertRaises(ValueError):m.http_ready('http://localhost:5173','0'*64,1,clock=clock,connect=lambda *a,**kw:conn)
        self.assertLessEqual(conn.recv.call_count,4);conn.close.assert_called_once()

    def test_caller_120_seconds_start_only_after_ready_and_stop_once(self):
        spec,_=fixture();spec['control_root']=str(self.root);packet=key_packets(spec)[0]
        a=workspace_reentry.command_info(packet)[1];a.pid_mailbox.parent.mkdir(parents=True,mode=0o700)
        params=dict(packet=packet,workspace_id='11111111-1111-4111-8111-111111111111',duration_seconds=120,receipts=[],transition=None)
        now=[0.];timings=[]
        @contextmanager
        def session(*args,**kw):
            box=args[5];record=synthetic(packet,nonce=box.nonce)
            _write(box.root,'pids.json',record['pid_raw'].encode());evidence={};box.session_evidence=evidence
            timings.append(('start',now[0]));yield evidence;timings.append(('stop',now[0]))
            pids=box.collect();evidence.update(control_plane_stop_verified=True,host_release_observations_complete=True,
                pid_handoff_collected=True,listed_pids=list(pids),service_id=workspace_reentry.service_record(record)[0]['id'],stop_response=record['stop_response'])
        def ready(*args):now[0]=5.4;timings.append(('http200',now[0]));return {'http_status':200}
        with patch.object(cli,'key_session',side_effect=session),patch.object(cli,'wait_ready',side_effect=ready),patch.object(cli.time,'monotonic',side_effect=lambda:now[0]),patch.object(cli.time,'sleep',side_effect=lambda d:now.__setitem__(0,now[0]+d)):
            result=cli.run(params,lambda:False)
        self.assertEqual(timings[0],('start',0.));self.assertAlmostEqual(timings[-1][1],125.4)
        self.assertTrue(Path(result['receipt']).is_file())


if __name__=='__main__':unittest.main()
