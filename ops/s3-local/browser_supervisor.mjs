// Bounded child of the Paperclip web command. Private CDP pipes, one fresh tab,
// fixed UI actions, no key extraction, user profile reuse, or arbitrary JS API.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {pathToFileURL} from 'node:url';
const hash=x=>crypto.createHash('sha256').update(x).digest('hex');
const nonce=()=>crypto.randomBytes(32).toString('hex');
const canonical=x=>Array.isArray(x)?x.map(canonical):x&&typeof x==='object'?Object.fromEntries(Object.keys(x).sort().map(k=>[k,canonical(x[k])])):x;
const encode=x=>JSON.stringify(canonical(x));
const save=(root,name,value)=>{
  const pending=path.join(root,'.'+name+'.pending');
  const fd=fs.openSync(pending,'wx',0o600);
  try{fs.writeFileSync(fd,encode(value)+'\n');fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
  fs.linkSync(pending,path.join(root,name));fs.unlinkSync(pending);
  const dir=fs.openSync(root,'r');try{fs.fsyncSync(dir);}finally{fs.closeSync(dir);}
};
export class Lifetime {
  constructor({request,parent,clock,limit=300000,emit}) {
    this.request=request;this.parent=parent;this.clock=clock;this.emit=emit;
    this.start=clock();this.last=this.start;this.limit=limit;this.document=nonce();
    this.state='WAIT_KEYS';this.registration=null;this.context=null;
  }
  fail(code){if(this.state!=='CLOSED'){this.state='FAILED';this.emit('failed',{code});}throw Error(code);}
  async observe(){
    const now=this.clock();if(now<this.last||now-this.start>=this.limit)return this.fail('DEADLINE');
    this.last=now;if(!this.parent())return this.fail('PARENT_LOST');
    if(['FAILED','CLOSED'].includes(this.state))throw Error('CLOSED');
    const challenge=nonce();let r;
    try{r=await this.request('observe',{challenge,document:this.document});}catch{return this.fail('TAB_LOST');}
    if(r.nonce!==challenge||r.document!==this.document||r.origin!=='http://127.0.0.1:5173'||r.closed)return this.fail('TAB_LOST');
    if(this.registration!==null&&hash(r.registration)!==this.registration)return this.fail('REGISTRATION_CHANGED');
    this.emit('observed',{nonce:challenge,document_nonce:this.document,state:this.state,registration_sha256:this.registration});
    return r;
  }
  async act(action,args={}){
    await this.observe();
    if(action==='prepare'){
      if(this.state!=='WAIT_KEYS')return this.fail('STATE');
      let r;try{r=await this.request('prepare',{});}catch{return this.fail('TAB_LOST');}
      // DOM projection only: the Wallet keeps private keys in this document.
      if(!r.registration||r.registration.length>32768)return this.fail('REGISTRATION');
      JSON.parse(r.registration);this.registration=hash(r.registration);this.state='KEYS_HELD';
      this.emit('keys_held',{registration_sha256:this.registration,registration:JSON.parse(r.registration)});
    }else if(action==='activate'){
      if(this.state!=='KEYS_HELD'||!args.context||hash(args.registration)!==this.registration)return this.fail('STATE');
      let observed;try{observed=await this.request('context',{});}catch{return this.fail('CONTEXT_CHANGED');}
      if(encode(observed)!==encode(args.context))return this.fail('CONTEXT_CHANGED');
      await this.observe();await this.request('activate',{});
      this.context=hash(encode(observed));this.state='ACTIVE';
      this.emit('active',{context_sha256:this.context,registration_sha256:this.registration});
    }else if(action==='inspect'){
      return {state:this.state,registration_sha256:this.registration,context_sha256:this.context};
    }else if(action==='finish'){this.state='CLOSED';return;}
    else if(this.state==='ACTIVE'&&['click','select','amount'].includes(action)){
      await this.request(action,args);
    }else return this.fail('STATE');
    if(this.state!=='CLOSED')await this.observe();
  }
}
class CDP {
  constructor(child){
    this.child=child;this.id=0;this.pending=new Map();this.buffer=Buffer.alloc(0);this.failed=false;
    child.stdio[4].on('data',raw=>{
      this.buffer=Buffer.concat([this.buffer,raw]);
      if(this.buffer.length>1048576){this.reject();return;}
      for(let i;(i=this.buffer.indexOf(0))>=0;){
        let v;try{v=JSON.parse(this.buffer.subarray(0,i));}catch{this.reject();return;}
        this.buffer=this.buffer.subarray(i+1);const p=this.pending.get(v.id);
        if(p){this.pending.delete(v.id);clearTimeout(p.timer);v.error?p.reject(Error('CDP')):p.resolve(v.result);}
        if(v.method==='Target.targetDestroyed'&&v.params.targetId===this.target)this.reject();
        if(v.method==='Page.frameNavigated'&&this.loader&&v.params.frame.parentId===undefined&&v.params.frame.loaderId!==this.loader)this.reject();
      }
    });
    child.on('exit',()=>this.reject());child.on('error',()=>this.reject());
    child.stdio[4].on('error',()=>this.reject());child.stdio[3].on('error',()=>this.reject());
  }
  reject(){this.failed=true;for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(Error('CDP_LOST'));}this.pending.clear();}
  call(method,params={},sessionId){
    if(this.failed)return Promise.reject(Error('CDP_LOST'));
    const id=++this.id;
    return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>{this.pending.delete(id);reject(Error('CDP_TIMEOUT'));},2000);
      this.pending.set(id,{resolve,reject,timer});
      this.child.stdio[3].write(encode({id,method,params,...(sessionId?{sessionId}:{})})+'\0');
    });
  }
  async evaluate(expression){
    const r=await this.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},this.session);
    if(r.exceptionDetails||!r.result||r.result.type==='error')throw Error('CDP_EVALUATION');
    return r.result.value;
  }
}
export async function main(argv){
  if(argv.length!==5)throw Error('ARGUMENTS');
  const [root,chrome,profile,parentText,lifetimeText]=argv;
  if(![root,chrome,profile].every(p=>path.isAbsolute(p)&&!p.split('/').includes('..'))||
      !/^[1-9][0-9]*$/.test(parentText)||!/^([1-9][0-9]{0,2})$/.test(lifetimeText)||+lifetimeText>300)throw Error('ARGUMENTS');
  const st=fs.lstatSync(root);if(!st.isDirectory()||(st.mode&0o777)!==0o700||st.uid!==process.getuid())throw Error('ROOT');
  fs.mkdirSync(profile,{mode:0o700});fs.mkdirSync(path.join(root,'commands'),{mode:0o700});fs.mkdirSync(path.join(root,'results'),{mode:0o700});
  let index=0,child,cdp,life,stopped=false;const seen=new Set();
  const emit=(phase,data={})=>save(root,String(index++).padStart(5,'0')+'.json',{
    schema:'s3-local-browser-lifetime/1',phase,pid:process.pid,parent_pid:+parentText,
    browser_pid:child?.pid??null,monotonic_ns:process.hrtime.bigint().toString(),...data,
    keys_exported:false,private_key_storage:false,cleanup_complete_verified:false});
  process.on('SIGINT',()=>{stopped=true;});process.on('SIGTERM',()=>{stopped=true;});
  try{
    child=spawn(chrome,['--headless=new','--remote-debugging-pipe','--no-first-run','--no-default-browser-check',
      '--disable-background-networking','--disable-sync','--disable-breakpad','--disable-crash-reporter',
      '--password-store=basic','--use-mock-keychain','--user-data-dir='+profile,'about:blank'],{
      stdio:['ignore','ignore','ignore','pipe','pipe'],env:{LANG:'C',LC_ALL:'C',PATH:'/usr/bin:/bin'}});
    cdp=new CDP(child);emit('spawned');
    const target=await cdp.call('Target.createTarget',{url:'http://127.0.0.1:5173/'});cdp.target=target.targetId;
    cdp.session=(await cdp.call('Target.attachToTarget',{targetId:cdp.target,flatten:true})).sessionId;
    await cdp.call('Page.enable',{},cdp.session);
    // Bounded readiness, before any keys. The parent has already bound the
    // exact approved static assets; no board wait is inside this process.
    const readyUntil=Date.now()+10000;
    while(!await cdp.evaluate('Boolean(document.getElementById("prepare"))')){
      if(stopped||Date.now()>=readyUntil)throw Error('PAGE_NOT_READY');await new Promise(r=>setTimeout(r,100));
    }
    cdp.loader=(await cdp.call('Page.getFrameTree',{},cdp.session)).frameTree.frame.loaderId;
    const request=async(action,a)=>{
      if(cdp.failed)throw Error('CDP_LOST');
      if(action==='observe')return cdp.evaluate(`({nonce:${encode(a.challenge)},document:globalThis.__nus_lifetime_document,origin:location.origin,closed:false,registration:document.getElementById('registration').textContent})`);
      if(action==='prepare')return cdp.evaluate(`(()=>{document.getElementById('enabled').checked=true;document.getElementById('acknowledge').checked=true;document.getElementById('prepare').click();return {registration:document.getElementById('registration').textContent};})()`);
      if(action==='context')return cdp.evaluate(`fetch('/runtime-context.json',{cache:'no-store',credentials:'omit',redirect:'error'}).then(r=>{if(!r.ok)throw Error();return r.json();})`);
      if(action==='activate'){
        await cdp.evaluate(`document.getElementById('activate').click()`);
        const until=Date.now()+3000;
        while(!await cdp.evaluate(`document.getElementById('wallet').querySelectorAll('button').length>0`)){
          if(Date.now()>=until)throw Error('ACTIVATION');await new Promise(r=>setTimeout(r,50));
        }
        return;
      }
      if(action==='click'&&['인증·조회','상태 재조회','출금 준비 (주문 동결·잔량 취소)','출금 준비 명시적 해제','직접 서명하여 출금','출금 결과 조회'].includes(a.name))return cdp.evaluate(`(()=>{const b=[...document.querySelectorAll('button')].filter(b=>b.textContent===${encode(a.name)});if(b.length!==1||b[0].disabled)throw Error();b[0].click();})()`);
      if(action==='select'&&['계정','출금 자산'].includes(a.name)&&['0','1','DEVBASE','DEVQUOTE'].includes(a.value))return cdp.evaluate(`(()=>{const l=[...document.querySelectorAll('label')].find(l=>l.textContent.startsWith(${encode(a.name)}));const s=l?.querySelector('select');if(!s)throw Error();s.value=${encode(a.value)};s.dispatchEvent(new Event('change',{bubbles:true}));})()`);
      if(action==='amount'&&typeof a.value==='string'&&/^\d{1,18}(\.\d{1,6})?$/.test(a.value))return cdp.evaluate(`(()=>{const l=[...document.querySelectorAll('label')].find(l=>l.textContent.startsWith('출금 금액'));const i=l?.querySelector('input');if(!i)throw Error();i.value=${encode(a.value)};i.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      throw Error('ACTION');
    };
    life=new Lifetime({request,parent:()=>process.ppid===+parentText&&!cdp.failed&&!stopped,clock:()=>Number(process.hrtime.bigint()/1000000n),limit:+lifetimeText*1000,emit});
    await cdp.evaluate(`Object.defineProperty(globalThis,'__nus_lifetime_document',{value:${encode(life.document)},writable:false,configurable:false})`);
    emit('tab_ready',{target_id:cdp.target,document_nonce:life.document});
    while(!stopped&&life.state!=='CLOSED'){
      await life.observe();
      const names=fs.readdirSync(path.join(root,'commands')).sort();if(names.length>1000)throw Error('COMMAND_LIMIT');
      for(const name of names){
        if(/^\.[0-9a-f]{64}\.json\.pending$/.test(name))continue;
        if(seen.has(name))continue;
        if(!/^[0-9a-f]{64}\.json$/.test(name))throw Error('COMMAND');
        const file=path.join(root,'commands',name),s=fs.lstatSync(file);
        if(!s.isFile()||s.isSymbolicLink()||s.nlink!==1||s.uid!==process.getuid()||(s.mode&0o777)!==0o600||s.size>65536)throw Error('COMMAND');
        const cmd=JSON.parse(fs.readFileSync(file,'utf8'));seen.add(name);
        if(cmd.nonce!==name.slice(0,-5)||!['prepare','inspect','activate','click','select','amount','finish'].includes(cmd.action))throw Error('COMMAND');
        if(cmd.action==='activate'){
          const contextFile=path.join(root,'active-context.json');
          const cs=fs.lstatSync(contextFile);if(!cs.isFile()||cs.isSymbolicLink()||cs.nlink!==1||cs.size>16384||(cs.mode&0o777)!==0o600)throw Error('CONTEXT');
          cmd.context=JSON.parse(fs.readFileSync(contextFile,'utf8'));
          cmd.registration=await cdp.evaluate("document.getElementById('registration').textContent");
        }
        const result=await life.act(cmd.action,cmd);
        if(cmd.action==='prepare')save(root,'registration.json',JSON.parse(await cdp.evaluate("document.getElementById('registration').textContent")));
        save(path.join(root,'results'),name,{nonce:cmd.nonce,action:cmd.action,state:life.state,result:result??null});
      }
      await new Promise(r=>setTimeout(r,500));
    }
    if(stopped)throw Error('STOPPED');
  }catch{emit('failed',{code:'BROWSER_LIFETIME_CLOSED'});process.exitCode=2;}
  finally{
    if(child){
      try{if(!cdp.failed)await cdp.call('Browser.close');}catch{}
      if(child.exitCode===null&&child.signalCode===null){child.kill('SIGTERM');await new Promise(r=>setTimeout(r,500));}
      if(child.exitCode===null&&child.signalCode===null)child.kill('SIGKILL');
      if(child.exitCode===null&&child.signalCode===null)await new Promise(r=>child.once('exit',r));
      emit('closed',{browser_exit:child.exitCode,keys_discarded:true});
    }
  }
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  main(process.argv.slice(2)).catch(()=>{process.stderr.write('LOCAL_BROWSER_REJECTED\n');process.exitCode=2;});
}
