"""One managed foreground owns preparation, browser and final web boundary.

The input-dependent transition is local and gate checked; there is no board
reconfiguration after keys. No source of keys/Context is substituted on failure.
"""
import base64
import hashlib
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import approval_gate
import key_preparation
import reviewed_web
from bootstrap_stage import _write
from manifest import decode
from pid_mailbox import _root, _write as write_record
from preflight import bounded, checked_root, verify, verify_input_set, MAX_MANIFEST
from web_proxy import WebProxy
from web_upstream import Upstream
from web_connection import serve_listener
from workspace_reentry import encode

SCRIPT='web/browser-supervisor.mjs'
RUNTIME='web/browser-runtime.json'


def assets(a):
    verify(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True)
    manifest=decode(bounded(checked_root(a.bundle),'runtime-manifest.json',MAX_MANIFEST))
    descriptor=decode(bounded(checked_root(a.bundle),'files/'+manifest['components']['sre'],8*1024*1024))
    inventory=decode(descriptor['implementation_settings']['artifacts_sha256_json'])
    captured={}
    for name,cap in ((SCRIPT,65536),(RUNTIME,1048576)):
        raw=bounded(checked_root(a.artifacts),name,cap)
        if hashlib.sha256(raw).hexdigest()!=inventory.get(name):raise ValueError('BROWSER_ASSET')
        captured[name]=raw
    runtime=decode(captured[RUNTIME])
    from browser_dependencies import verify as dependencies
    dependencies(runtime)
    return captured[SCRIPT],runtime


class SwitchingWeb(WebProxy):
    def __init__(self,preparation,activate):
        super().__init__(origin=preparation.origin,worker_port=preparation.destination[1],static=preparation.static)
        self.boundary=preparation;self.activate=activate;self.active=False;self.attempted=False
    def transition(self):
        if self.attempted:raise ValueError('BROWSER_TRANSITION_USED')
        self.attempted=True
        proxy=self.activate()
        if any(proxy.static._routes[k]!=self.static._routes[k] for k in ('/','/page.js')):
            raise ValueError('BROWSER_ASSETS_CHANGED')
        self.boundary=proxy;self.active=True
    def respond(self,*args):return self.boundary.respond(*args)


def run(a,arguments,origin,port,values,decision,revisions,stop):
    control=Path(a.browser_control);_root(control.parent)
    # New evidence directory, no background/reuse or deletion of an old tab.
    control.mkdir(mode=0o700)
    def audit():
        if stop():raise ValueError('STOPPED')
        return approval_gate.inspect(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True,decision,revisions)
    baseline=audit();script,runtime=assets(a)
    preparation=key_preparation.prepare(a.bundle,a.artifacts,a.runtime_pin,origin)
    def activate():
        if audit()!=baseline:raise ValueError('BROWSER_APPROVAL_CHANGED')
        prepared=reviewed_web.prepare(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True,
            decision,revisions,a.input_set.parent,a.input_set.name,arguments,a.scratch,origin)
        raw,_=verify_input_set(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True,a.input_set.parent,a.input_set.name)
        if hashlib.sha256(raw).hexdigest()!=prepared.capture_sha256:raise ValueError('CAPTURE_CHANGED')
        capture=decode(raw);genesis=decode(base64.b64decode(capture['genesis'],validate=True))
        registrations=decode(bounded(control,'registration.json',32768))
        if (not isinstance(registrations,list) or len(registrations)!=2 or
                [r['public_key_base64'] for r in registrations]!=genesis['app_state']['public_keys']):
            raise ValueError('BROWSER_GENESIS_REGISTRATION')
        if audit()!=baseline or stop():raise ValueError('BROWSER_APPROVAL_CHANGED')
        context=decode(prepared.response_boundary._routes['/runtime-context.json'][1])
        from browser_control import publish
        publish(control,'active-context.json',encode(context))
        return WebProxy(origin=origin,worker_port=port,static=prepared.response_boundary)
    proxy=SwitchingWeb(preparation,activate)
    child=None;listener=None;started=time.monotonic();primary=None;phase='private_stage'
    with tempfile.TemporaryDirectory(prefix='managed-browser-',dir=checked_root(a.scratch)) as temp:
        temp=Path(temp);staged=temp/'browser-supervisor.mjs';_write(staged,script,0o500)
        try:
            phase='approval_recheck'
            if audit()!=baseline:raise ValueError('BROWSER_APPROVAL_CHANGED')
            phase='loopback_bind'
            listener=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
            listener.bind(('127.0.0.1',5173));listener.listen(1)
            phase='node_spawn'
            child=subprocess.Popen([runtime['node']['path'],str(staged),str(control),runtime['chrome']['path'],
                str(temp/'chrome-profile'),str(os.getpid()),str(values[0])],stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,close_fds=True,start_new_session=True,
                env={'PATH':'/usr/bin:/bin','LANG':'C','LC_ALL':'C'})
            write_record(control,'parent.json',encode(dict(schema='s3-local-browser-parent/1',
                parent_pid=os.getpid(),node_pid=child.pid,script_sha256=hashlib.sha256(script).hexdigest(),
                runtime_pin=a.runtime_pin,started_monotonic_ns=time.monotonic_ns(),lifetime_seconds=values[0])))
            def ended():
                if stop() or time.monotonic()-started>=values[0]:return True
                if child.poll() is not None:
                    if child.returncode!=0:raise ValueError('BROWSER_CHILD_EXITED')
                    return True
                return False
            def transition():
                if not proxy.attempted:
                    try:(control/'activate.json').lstat()
                    except FileNotFoundError:pass
                    else:
                        marker=decode(bounded(control,'activate.json',4096))
                        if marker!={'schema':'s3-local-browser-transition/1','runtime_pin':a.runtime_pin}:
                            raise ValueError('BROWSER_TRANSITION')
                        proxy.transition()
            if ended():raise ValueError('STOPPED')
            phase='http_serve'
            return serve_listener(listener,proxy,Upstream(),max_requests=values[1],
                lifetime_seconds=values[0],stop=ended,between_requests=transition)
        except BaseException as error:
            primary=error
            raise
        finally:
            if listener is not None:listener.close()
            if child is not None:
                # The managed child and Chrome share our owned group. Do not
                # use a reaped/reused PID to issue a later group signal.
                if child.poll() is None:
                    try:os.killpg(child.pid,signal.SIGTERM)
                    except ProcessLookupError:pass
                    try:child.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        try:os.killpg(child.pid,signal.SIGKILL)
                        except ProcessLookupError:pass
                        child.wait(timeout=3)
                write_record(control,'parent-closed.json',encode(dict(node_pid=child.pid,exit_code=child.returncode,
                    parent_error=primary is not None,parent_failed_phase=phase if primary is not None else None,
                    parent_error_code='BROWSER_PARENT_FAILED' if primary is not None else None,
                    whole_process_tree_verified=False,cleanup_complete_verified=False)))
