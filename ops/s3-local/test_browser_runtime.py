"""Real Node/process/pipes with a synthetic CDP child. No browser or listener."""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock,patch
import browser_control
import browser_inventory
import browser_web
from pid_mailbox import _write
from workspace_reentry import encode

NODE='/opt/homebrew/Cellar/node@24/24.21.0/bin/node'
FAKE=r'''
import os,json,re
buf=b'';doc=None;reg='';active=False
while True:
 raw=os.read(3,65536)
 if not raw:break
 buf+=raw
 while b'\0' in buf:
  line,_,buf=buf.partition(b'\0');r=json.loads(line);m=r['method'];value={}
  if m=='Target.createTarget':value={'targetId':'synthetic-tab'}
  elif m=='Target.attachToTarget':value={'sessionId':'synthetic-session'}
  elif m=='Page.getFrameTree':value={'frameTree':{'frame':{'id':'same-frame','loaderId':'same-document'}}}
  elif m=='Browser.close':os._exit(0)
  elif m=='Runtime.evaluate':
   e=r['params']['expression'];v=None
   if e.startswith('Boolean('):v=True
   elif e.startswith('Object.defineProperty'):doc=re.search('value:"([0-9a-f]+)"',e).group(1)
   elif e.startswith('({nonce:'):
    v={'nonce':re.search('nonce:"([0-9a-f]+)"',e).group(1),'document':doc,'origin':'http://127.0.0.1:5173','closed':False,'registration':reg}
   elif "getElementById('prepare').click" in e:
    reg=json.dumps([{'public_key_base64':'public-a'},{'public_key_base64':'public-b'}]);v={'registration':reg}
   elif e=="document.getElementById('registration').textContent":v=reg
   elif "fetch('/runtime-context.json'" in e:v={'status':200,'ok':True,'context':{'chain_id':'synthetic','genesis_hash':'a'*64}}
   elif "getElementById('activate').click" in e:active=True
   elif "querySelectorAll('button').length" in e:v=active
   value={'result':{'type':'boolean' if isinstance(v,bool) else 'object','value':v}}
  os.write(4,(json.dumps({'id':r['id'],'result':value})+'\0').encode())
'''


class BrowserRuntimeTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.control=self.root/'control';self.control.mkdir(mode=0o700)
        self.chrome=self.root/'synthetic-cdp';self.chrome.write_text('#!'+sys.executable+'\n'+FAKE);self.chrome.chmod(0o500)
        self.child=None
        self.addCleanup(self.cleanup)

    def cleanup(self):
        if self.child and self.child.poll() is None:self.child.terminate();self.child.wait(timeout=5)

    def start(self,lifetime=30,parent=None):
        self.child=subprocess.Popen([NODE,str(Path(__file__).with_name('browser_supervisor.mjs')),
            str(self.control),str(self.chrome),str(self.root/'profile'),str(os.getpid() if parent is None else parent),str(lifetime)],
            stdout=subprocess.PIPE,stderr=subprocess.PIPE,stdin=subprocess.DEVNULL)
        _write(self.control,'parent.json',encode(dict(schema='s3-local-browser-parent/1',parent_pid=os.getpid() if parent is None else parent,node_pid=self.child.pid)))

    def close_record(self):
        out,err=self.child.communicate(timeout=5)
        self.assertEqual(out,b'');self.assertEqual(err,b'')
        _write(self.control,'parent-closed.json',encode(dict(node_pid=self.child.pid,exit_code=self.child.returncode,parent_error=False)))
        return browser_inventory.read(self.control)

    def test_real_private_pipes_same_tab_lifetime_and_three_pid_inventory(self):
        self.start();self.assertEqual(browser_control.wait_ready(self.control)['state'],'WAIT_KEYS')
        self.assertEqual(browser_control.command(self.control,'prepare')['state'],'KEYS_HELD')
        public=json.loads((self.control/'registration.json').read_bytes());self.assertEqual(len(public),2)
        browser_control.publish(self.control,'active-context.json',encode(dict(chain_id='synthetic',genesis_hash='a'*64)))
        self.assertEqual(browser_control.command(self.control,'activate')['state'],'ACTIVE')
        self.assertEqual(browser_control.command(self.control,'finish')['state'],'CLOSED')
        inventory=self.close_record();self.assertTrue(inventory['normal_session'])
        self.assertEqual(inventory['pids'][:2],[os.getpid(),self.child.pid]);self.assertFalse(inventory['cleanup_complete_verified'])
        self.assertFalse(any('private_key' in x for x in json.dumps(public).split()))
        closed=json.loads((self.control/'parent-closed.json').read_bytes());closed['node_pid']+=1
        (self.control/'parent-closed.json').write_bytes(encode(closed))
        with self.assertRaises(ValueError):browser_inventory.read(self.control)

    def test_disconnect_parent_loss_and_signal_record_failure_close_child(self):
        for mode in ('parent','signal','disconnect'):
            with self.subTest(mode=mode):
                if mode!='parent':
                    self.control=self.root/mode;self.control.mkdir(mode=0o700)
                    profile=self.root/'profile'
                    if profile.exists():profile.rename(self.root/('old-'+mode))
                self.start(parent=os.getpid()+12345 if mode=='parent' else None)
                if mode!='parent':
                    browser_control.wait_ready(self.control)
                    if mode=='signal':self.child.terminate()
                    else:
                        first=json.loads((self.control/'00000.json').read_bytes())
                        os.kill(first['browser_pid'],15) # Owned synthetic child only.
                r=self.close_record();self.assertFalse(r['normal_session']);self.assertEqual(self.child.returncode,2)
                for pid in r['pids'][2:]:
                    with self.assertRaises(ProcessLookupError):os.kill(pid,0)

    def test_actual_main_first_failure_phase_and_method_no_secret_or_cleanup_overwrite(self):
        cases=[('Target.createTarget','target_create','CDP_RESPONSE'),
               ('Target.attachToTarget','target_attach','CDP_RESPONSE'),
               ('Page.enable','page_enable','CDP_RESPONSE'),
               ('Runtime.evaluate','page_readiness','CDP_RESPONSE'),
               ('Page.getFrameTree','frame_identity','CDP_RESPONSE')]
        for i,(method,phase,code) in enumerate(cases):
            with self.subTest(method=method):
                self.control=self.root/f'fault{i}';self.control.mkdir(mode=0o700)
                profile=self.root/'profile'
                if profile.exists():profile.rename(self.root/f'profile-{i}')
                injected=FAKE.replace("m=r['method'];value={}","m=r['method'];value={}\n  if m=="+repr(method)+":\n   os.write(4,(json.dumps({'id':r['id'],'error':{'code':-32000,'message':'SECRET-DO-NOT-LOG','data':'private body'}})+'\\0').encode());continue")
                self.chrome.chmod(0o700);self.chrome.write_text('#!'+sys.executable+'\n'+injected);self.chrome.chmod(0o500)
                self.start();result=self.close_record();self.assertFalse(result['normal_session'])
                events=[json.loads(p.read_text()) for p in sorted(self.control.glob('[0-9]*.json'))]
                failures=[e for e in events if e['phase']=='failed'];self.assertEqual(len(failures),1)
                self.assertEqual((failures[0]['failed_phase'],failures[0]['method'],failures[0]['code']),(phase,method,code))
                self.assertEqual(failures[0]['cdp_error_code'],-32000);self.assertNotIn('SECRET',json.dumps(events));self.assertNotIn('private body',json.dumps(events))
                self.assertEqual(events[-1]['phase'],'closed');self.assertIn('browser_signal',events[-1])
                self.assertFalse(any(e['phase'] in ('keys_held','active') for e in events))
                with self.assertRaises(ProcessLookupError):os.kill(result['pids'][2],0)

    def test_bad_cdp_framing_and_evaluation_are_distinct_and_reaped(self):
        for i,(payload,code) in enumerate([("b'null\\0'",'CDP_FORMAT'),("b'{bad\\0'",'CDP_JSON'),("b'x'*2097153",'CDP_SIZE'),
              ("(json.dumps({'id':r['id'],'result':{'exceptionDetails':{'text':'SECRET'}}})+'\\0').encode()",'CDP_EVALUATION')]):
            with self.subTest(code=code):
                self.control=self.root/f'frame{i}';self.control.mkdir(mode=0o700)
                profile=self.root/'profile'
                if profile.exists():profile.rename(self.root/f'old-frame-{i}')
                injected=FAKE.replace("m=r['method'];value={}","m=r['method'];value={}\n  if m=='Runtime.evaluate':\n   os.write(4,"+payload+");continue")
                self.chrome.chmod(0o700);self.chrome.write_text('#!'+sys.executable+'\n'+injected);self.chrome.chmod(0o500)
                self.start();self.close_record()
                events=[json.loads(p.read_text()) for p in sorted(self.control.glob('[0-9]*.json'))]
                first=next(e for e in events if e['phase']=='failed')
                self.assertEqual(first['code'],code);self.assertEqual(first['failed_phase'],'page_readiness')
                self.assertNotIn('SECRET',json.dumps(events));self.assertEqual(self.child.returncode,2)

    def test_context_http_status_is_diagnostic_and_first_failure_survives_close(self):
        modified=FAKE.replace("'status':200,'ok':True", "'status':503,'ok':False")
        self.chrome.chmod(0o700);self.chrome.write_text('#!'+sys.executable+'\n'+modified);self.chrome.chmod(0o500)
        self.start();browser_control.wait_ready(self.control);browser_control.command(self.control,'prepare')
        browser_control.publish(self.control,'active-context.json',encode(dict(chain_id='synthetic',genesis_hash='a'*64)))
        with self.assertRaises(ValueError):browser_control.command(self.control,'activate',seconds=1)
        self.close_record()
        events=[json.loads(p.read_text()) for p in sorted(self.control.glob('[0-9]*.json'))]
        first=next(e for e in events if e['phase']=='failed')
        self.assertEqual((first['failed_phase'],first['code'],first['http_status']),('ui_context','CONTEXT_HTTP',503))
        self.assertFalse(any(e['phase']=='active' for e in events))

    def test_active_screenshot_has_bounded_owned_path_and_no_caller_script(self):
        modified=FAKE.replace("elif m=='Browser.close':os._exit(0)","elif m=='Page.captureScreenshot':value={'data':'iVBORw0KGgo='}\n  elif m=='Browser.close':os._exit(0)")
        self.chrome.chmod(0o700);self.chrome.write_text('#!'+sys.executable+'\n'+modified);self.chrome.chmod(0o500)
        self.start();browser_control.wait_ready(self.control);browser_control.command(self.control,'prepare')
        browser_control.publish(self.control,'active-context.json',encode(dict(chain_id='synthetic',genesis_hash='a'*64)))
        browser_control.command(self.control,'activate')
        result=browser_control.command(self.control,'screenshot')['result'];path=self.control/result['file']
        self.assertEqual(path.parent,self.control/'screenshots');self.assertEqual(path.stat().st_mode&0o777,0o600)
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),result['sha256']);self.assertEqual(result['bytes'],8)
        with self.assertRaises(ValueError):browser_control.command(self.control,'screenshot',{'path':'/external'})
        for _ in range(15):browser_control.command(self.control,'screenshot')
        with self.assertRaises(ValueError):browser_control.command(self.control,'screenshot',seconds=1)
        self.assertFalse(self.close_record()['normal_session'])
        self.assertEqual(len(list((self.control/'screenshots').iterdir())),16)
        events=[json.loads(p.read_text()) for p in self.control.glob('[0-9]*.json')]
        self.assertEqual(next(e for e in events if e['phase']=='failed')['code'],'SCREENSHOT_LIMIT')

    def test_screen_wait_only_observes_fixed_owner_and_bounded_public_fields(self):
        base=dict(schema='s3-local-browser-screen/1',identity='계정 alice · revision 1',status='보류')
        done=dict(base,identity='계정 alice · revision 2',status='조회 정상')
        with patch.object(browser_control,'command',side_effect=[{'result':base},{'result':done}]) as call,patch.object(browser_control.time,'sleep'):
            self.assertEqual(browser_control.wait_screen(self.control,'status','조회 정상'),done)
            self.assertEqual([x.args[1] for x in call.call_args_list],['snapshot','snapshot'])
        with patch.object(browser_control,'command',side_effect=[{'result':base},{'result':dict(done,identity='계정 bob · revision 2')}]),patch.object(browser_control.time,'sleep'),self.assertRaisesRegex(ValueError,'IDENTITY'):
            browser_control.wait_screen(self.control,'status','조회 정상')
        with patch.object(browser_control,'command') as call,self.assertRaises(ValueError):browser_control.wait_screen(self.control,'key','secret')
        call.assert_not_called()

    def test_switch_only_once_same_assets_and_error_preserves_preparation(self):
        from web_proxy import WebProxy
        from static_web import StaticWeb
        # The response boundary shape is sufficient for the switch; the
        # original classes still own Origin/auth/routing in their own tests.
        class Assets(StaticWeb):
            def __init__(self,javascript=b'js'):
                super().__init__(origin='http://127.0.0.1:5173',html=b'html',javascript=javascript)
        preparation=Mock(origin='http://127.0.0.1:5173',destination=('127.0.0.1',18080),static=Assets())
        good=WebProxy(origin='http://127.0.0.1:5173',worker_port=18080,static=Assets())
        for cause in ('good','changed','denied'):
            factory=Mock(return_value=good)
            if cause=='changed':
                other=Assets(b'changed')
                factory.return_value=WebProxy(origin='http://127.0.0.1:5173',worker_port=18080,static=other)
            if cause=='denied':factory.side_effect=ValueError('denied')
            boundary=browser_web.SwitchingWeb(preparation,factory)
            if cause=='good':boundary.transition();self.assertTrue(boundary.active)
            else:
                with self.assertRaises(ValueError):boundary.transition()
                self.assertIs(boundary.boundary,preparation);self.assertFalse(boundary.active)
            with self.assertRaises(ValueError):boundary.transition()
            factory.assert_called_once()

    def test_complete_existing_browser_dependencies_reject_drift_and_external_link(self):
        import browser_dependencies as dep
        node=self.root/'node';node.write_bytes(b'synthetic-node')
        contents=self.root/'Chrome.app/Contents';(contents/'MacOS').mkdir(parents=True)
        chrome=contents/'MacOS/Google Chrome';chrome.write_bytes(b'synthetic-chrome')
        resource=contents/'resource';resource.write_bytes(b'resource')
        (contents/'alias').symlink_to('resource')
        value=dep.capture(node,chrome);dep.verify(value)
        resource.write_bytes(b'changed')
        with self.assertRaises(ValueError):dep.verify(value)
        (contents/'external').symlink_to(node)
        with self.assertRaises(ValueError):dep.capture(node,chrome)


if __name__=='__main__':unittest.main()
