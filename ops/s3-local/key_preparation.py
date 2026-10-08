"""Explicit pre-genesis web phase. Same approved UI/key code, public assets only.

No Context, C store, authentication, RPC, proxy or private-key import/export.
L-T must start this through Paperclip, then stop it while retaining the tab.
"""
import hashlib
import socket
import approval_gate
from manifest import decode
from preflight import verify,bounded,checked_root,MAX_MANIFEST,MAX_DESCRIPTOR
from static_web import StaticWeb
from web_proxy import WebProxy
from web_connection import serve_listener


class KeyPreparation(WebProxy):
    def respond(self,method,target,headers,body,peer,exchange):
        if body or target not in ('/','/page.js'):
            return self.failure(404)
        return self.static.respond(method,target,headers,peer)


def prepare(bundle,artifacts,pin,origin):
    verify(bundle,artifacts,pin,'s3-dev-local/1',True)
    manifest=decode(bounded(checked_root(bundle),'runtime-manifest.json',MAX_MANIFEST))
    path=manifest['components']['sre']
    descriptor=bounded(checked_root(bundle),'files/'+path,MAX_DESCRIPTOR)
    if hashlib.sha256(descriptor).hexdigest()!=manifest['files_sha256'][path]:raise ValueError('DESCRIPTOR_CHANGED')
    inventory=decode(decode(descriptor)['implementation_settings']['artifacts_sha256_json'])
    files={}
    for name,limit in [('web/index.html',65536),('web/page.js',4194304)]:
        raw=bounded(checked_root(artifacts),name,limit)
        if hashlib.sha256(raw).hexdigest()!=inventory.get(name):raise ValueError('ASSET_CHANGED')
        files[name]=raw
    return KeyPreparation(origin=origin,worker_port=18080,static=StaticWeb(origin=origin,
                           html=files['web/index.html'],javascript=files['web/page.js']))


def run(bundle,artifacts,pin,origin,decision,revisions,stop,on_ready,*,socket_factory=socket.socket):
    def audit():return approval_gate.inspect(bundle,artifacts,pin,'s3-dev-local/1',True,decision,revisions)
    if stop():raise ValueError('STOPPED')
    before=audit();proxy=prepare(bundle,artifacts,pin,origin)
    if audit()!=before or stop():raise ValueError('APPROVAL_CHANGED')
    def no_upstream(*args,**kwargs):raise AssertionError('KEY_PREPARATION_HAS_NO_UPSTREAM')
    listener=socket_factory(socket.AF_INET,socket.SOCK_STREAM)
    try:
        if stop():raise ValueError('STOPPED')
        on_ready();listener.bind(('127.0.0.1',5173));listener.listen(1)
        return serve_listener(listener,proxy,no_upstream,max_requests=100,lifetime_seconds=300,stop=stop)
    finally:listener.close()
