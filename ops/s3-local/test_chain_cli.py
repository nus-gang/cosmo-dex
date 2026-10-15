import contextlib
import io
import subprocess
import sys
import unittest
from unittest.mock import patch
import chain_cli as c

class ChainCliTest(unittest.TestCase):
    def args(self):
        a=['serve-chain-reviewed','--acknowledge-unproven-space']
        for n in ('bundle','artifacts','input-set','effective-profile','home','scratch','approval-socket','pid-mailbox'):
            a += ['--'+n, '/private/'+n]
        for n,v in dict(runtime_pin='a'*64,local_demo_profile='s3-dev-local/1',native_decision_id='11111111-1111-4111-8111-111111111111',ceo_revision='22222222-2222-4222-8222-222222222222',cto_revision='33333333-3333-4333-8333-333333333333',rpc='127.0.0.1:26657',p2p='127.0.0.1:26656',peers=','.join(str(i)*40+'@127.0.0.1:'+str(27000+i) for i in range(1,4)),lifetime_seconds='300').items():
            a += ['--'+n.replace('_','-'),v]
        return a
    def invoke(self,a):
        out,err=io.StringIO(),io.StringIO()
        with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err): code=c.main(a)
        return code,out.getvalue(),err.getvalue()
    def test_scopes_and_exact_forwarding(self):
        events=[]
        @contextlib.contextmanager
        def scope(name,value=None):
            events.append(name)
            try: yield value
            finally: events.append(name+'-close')
        report=dict(start_sent=True,stop_requested=False,child_exit=0,output_bytes=0,approval_verified=False,reusable_permit=False,cleanup_complete_verified=False)
        record=lambda pid:None
        def run(*a,**kw):
            self.assertEqual(a[:6],('staged','a'*64,'/private/home','127.0.0.1:26657','127.0.0.1:26656',c.parse(self.args()).peers))
            self.assertEqual(a[6](),{'fresh':True})
            self.assertIs(kw['on_spawn'],record); self.assertFalse(kw['stopped']())
            self.assertEqual(kw['lifetime'],300)
            return report
        with patch.object(c,'reporter',return_value=record), patch.object(c,'stop_latch',side_effect=lambda:scope('signal',lambda:False)), patch.object(c.approval_gate,'private_transport',side_effect=lambda p:scope('reader')), patch.object(c.chain_stage,'stage',side_effect=lambda *a:scope('stage','staged')), patch.object(c.approval_gate,'inspect',return_value={'fresh':True}), patch.object(c.chain_run,'run',side_effect=run):
            self.assertEqual(self.invoke(self.args()),(0,'',''))
        self.assertEqual(events,['signal','reader','stage','stage-close','reader-close','signal-close'])
    def test_bad_arguments_no_effect(self):
        good=self.args(); cases=[[],good+['--rpc','127.0.0.1:3333'],good+['--rp','x'],[x for x in good if x!='--acknowledge-unproven-space']]
        for opt,values in {'--rpc':['0.0.0.0:26657','127.0.0.1:26656'],'--peers':['','a'*40+'@127.0.0.1:1'],'--lifetime-seconds':['301','01','+1'],'--runtime-pin':['x'],'--home':['relative','/x/../y']}.items():
            for value in values:
                a=good.copy();a[a.index(opt)+1]=value;cases.append(a)
        with patch.object(c,'reporter') as record:
            for a in cases:self.assertEqual(self.invoke(a),(2,'','LOCAL_MANAGED_CHAIN_REJECTED\n'))
            record.assert_not_called()
    def test_errors_restore_and_hide_details(self):
        for failure in (ValueError('secret'),OSError('secret'),KeyboardInterrupt()):
            with patch.object(c,'reporter',return_value=lambda p:None),patch.object(c.approval_gate,'private_transport',return_value=contextlib.nullcontext()),patch.object(c.chain_stage,'stage',return_value=contextlib.nullcontext('staged')),patch.object(c.chain_run,'run',side_effect=failure):
                self.assertEqual(self.invoke(self.args()),(2,'','LOCAL_MANAGED_CHAIN_REJECTED\n'))
    def test_open_stdin_immediate_reject(self):
        p=subprocess.Popen([sys.executable,'-B',c.__file__,'serve-chain-reviewed'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertEqual(p.wait(timeout=3),2);self.assertEqual(p.stdout.read(),b'');self.assertEqual(p.stderr.read(),b'LOCAL_MANAGED_CHAIN_REJECTED\n')
        finally:
            if p.poll() is None:p.kill()
            p.wait()
            for s in (p.stdin,p.stdout,p.stderr):s.close()
