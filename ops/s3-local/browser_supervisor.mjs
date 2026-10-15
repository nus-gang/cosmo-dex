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
const failure=(code,details={})=>Object.assign(Error(code),{code,details});
const id=x=>typeof x==='string'&&/^[a-zA-Z0-9_-]{1,128}$/.test(x)?x:null;
const clock=()=>Number(process.hrtime.bigint()/1000000n);
// Never serialize exception messages, arbitrary CDP bodies, JS or credentials.
export class Diagnostic {
  constructor(emit){this.emit=emit;this.completed=null;this.current='initialization';this.first=null;}
  async step(phase,fn){this.current=phase;const value=await fn();this.completed=phase;return value;}
  fail(error,ids={}){
    if(this.first)return;
    this.first={code:error?.code??'UNCLASSIFIED',failed_phase:this.current,last_completed_phase:this.completed,
      ...ids,...(error?.code?error.details:{})};
    this.emit('failed',this.first);
  }
}
// The expressions are closed over this reviewed DOM contract, never caller JS.
export function uiExpression(action,a={}){
  const buttons=['인증·조회','상태 재조회','직접 서명하여 예치','주문 서명·제출','잔량 취소 서명·제출',
    '주문·취소 원 결과 조회','출금 준비 (주문 동결·잔량 취소)','출금 준비 명시적 해제','직접 서명하여 출금','출금 결과 조회'];
  const selects={'계정':['0','1'],'출금 자산':['DEVBASE','DEVQUOTE'],'예치 자산':['DEVBASE','DEVQUOTE'],
    '주문 방향':['SELL','BUY'],'주문 유형':['GTC','IOC'],'최대 수수료 bps':['0','25']};
  const fields={'출금 금액':/^(0|[1-9][0-9]{0,17})(\.[0-9]{1,6})?$/,'예치 금액':/^(0|[1-9][0-9]{0,17})(\.[0-9]{1,6})?$/,
    '주문 수량 lots':/^[1-9][0-9]{0,19}$/,'지정가 ticks':/^[1-9][0-9]{0,19}$/};
  if(action==='amount'){action='input';a={name:'출금 금액',value:a.value};}
  const valid=action==='click'?buttons.includes(a.name):action==='select'?
    (selects[a.name]?.includes(a.value)||(a.name==='취소 주문'&&typeof a.value==='string'&&/^[0-9a-f]{64}:[0-9a-f]{64}$/.test(a.value))):
    action==='input'?typeof a.value==='string'&&fields[a.name]?.test(a.value):action==='snapshot';
  if(!valid)throw failure('UI_ARGUMENT');
  // Only public DOM text from the mounted Wallet, never globals/client/key state.
  function perform(action,a){
    const root=document.getElementById('wallet');if(!root)throw Error('UI_STRUCTURE');
    const all=selector=>[...root.querySelectorAll(selector)];
    const one=items=>{if(items.length!==1)throw Error('UI_TARGET');return items[0];};
    if(action==='snapshot'){
      const text=e=>{const s=e.textContent??'';if(s.length>16384)throw Error('UI_SIZE');return s;};
      const children=[...root.children];if(children.length>64)throw Error('UI_SIZE');
      const table=one(all('table')),status=one(all('[role="status"]'));
      const sections=all('div').map(e=>[...e.children].map(text));
      if(sections.length!==5||sections.some(s=>s.length>1024))throw Error('UI_STRUCTURE');
      const result={schema:'s3-local-browser-screen/1',status:text(status),
        identity:text(one(children.filter(e=>e.tagName==='P'&&e.textContent.startsWith('계정 ')))),
        ledger:[...table.querySelectorAll('tbody tr')].map(r=>[...r.children].map(text)),
        commands:sections[0],orders:sections[1],fills:sections[2],batches:sections[3],history:sections[4],
        receipts:children.filter(e=>e.tagName==='P').map(text),
        buttons:all('button').map(e=>({name:text(e),disabled:e.disabled})),
        selections:all('select').map(e=>({name:e.getAttribute('aria-label'),value:e.value,options:[...e.options].map(o=>({value:o.value,text:text(o)}))}))};
      if(new TextEncoder().encode(JSON.stringify(result)).length>48000)throw Error('UI_SIZE');return result;
    }
    const target=one(action==='click'?all('button').filter(e=>e.textContent===a.name):
      all(action==='select'?'select':'input').filter(e=>e.getAttribute('aria-label')===a.name));
    if(target.disabled||target.readOnly)throw Error('UI_DISABLED');
    if(action==='click'){target.click();return {dispatched:true};}
    if(action==='select'&&![...target.options].some(o=>o.value===a.value&&!o.disabled))throw Error('UI_OPTION');
    target.value=a.value;if(target.value!==a.value)throw Error('UI_VALUE');
    target.dispatchEvent(new Event(action==='select'?'change':'input',{bubbles:true}));
    return {dispatched:true};
  }
  return `(${perform.toString()})(${encode(action)},${encode(a)})`;
}
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
  fail(code){if(this.state!=='CLOSED')this.state='FAILED';throw failure(code);}
  async observe(){
    const now=this.clock();if(now<this.last||now-this.start>=this.limit)return this.fail('DEADLINE');
    this.last=now;if(!this.parent())return this.fail('PARENT_LOST');
    if(['FAILED','CLOSED'].includes(this.state))throw Error('CLOSED');
    const challenge=nonce();let r;
    r=await this.request('observe',{challenge,document:this.document});
    if(r.nonce!==challenge||r.document!==this.document||r.origin!=='http://127.0.0.1:5173'||r.closed)return this.fail('TAB_LOST');
    if(this.registration!==null&&hash(r.registration)!==this.registration)return this.fail('REGISTRATION_CHANGED');
    this.emit('observed',{nonce:challenge,document_nonce:this.document,state:this.state,registration_sha256:this.registration});
    return r;
  }
  async act(action,args={}){
    try{return await this._act(action,args);}catch(e){if(this.state!=='CLOSED')this.state='FAILED';throw e;}
  }
  async _act(action,args={}){
    await this.observe();
    if(action==='prepare'){
      if(this.state!=='WAIT_KEYS')return this.fail('STATE');
      const r=await this.request('prepare',{});
      // DOM projection only: the Wallet keeps private keys in this document.
      if(!r.registration||r.registration.length>32768)return this.fail('REGISTRATION');
      JSON.parse(r.registration);this.registration=hash(r.registration);this.state='KEYS_HELD';
      this.emit('keys_held',{registration_sha256:this.registration,registration:JSON.parse(r.registration)});
    }else if(action==='activate'){
      if(this.state!=='KEYS_HELD'||!args.context||hash(args.registration)!==this.registration)return this.fail('STATE');
      const observed=await this.request('context',{});
      if(encode(observed)!==encode(args.context))return this.fail('CONTEXT_CHANGED');
      await this.observe();await this.request('activate',{});
      this.context=hash(encode(observed));this.state='ACTIVE';
      this.emit('active',{context_sha256:this.context,registration_sha256:this.registration});
    }else if(action==='inspect'){
      return {state:this.state,registration_sha256:this.registration,context_sha256:this.context};
    }else if(action==='finish'){this.state='CLOSED';return;}
    else if(this.state==='ACTIVE'&&['click','select','amount','input','snapshot','screenshot'].includes(action)){
      const result=await this.request(action,args);await this.observe();return result;
    }else return this.fail('STATE');
    if(this.state!=='CLOSED')await this.observe();
  }
}
export class CDP {
  constructor(child){
    this.child=child;this.id=0;this.pending=new Map();this.buffer=Buffer.alloc(0);this.failed=false;
    child.stdio[4].on('data',raw=>{
      this.buffer=Buffer.concat([this.buffer,raw]);
      if(this.buffer.length>2097152){this.reject('CDP_SIZE');return;}
      for(let i;(i=this.buffer.indexOf(0))>=0;){
        let v;try{v=JSON.parse(this.buffer.subarray(0,i));}catch{this.reject('CDP_JSON');return;}
        this.buffer=this.buffer.subarray(i+1);
        if(!v||typeof v!=='object'){this.reject('CDP_FORMAT');return;}
        const p=this.pending.get(v.id);
        if(p){this.pending.delete(v.id);clearTimeout(p.timer);v.error?p.reject(failure('CDP_RESPONSE',{method:p.method,cdp_error_code:Number.isSafeInteger(v.error.code)?v.error.code:null})):p.resolve(v.result);}
        if(v.method==='Target.targetDestroyed'&&v.params?.targetId===this.target)this.reject();
        if(v.method==='Page.frameNavigated'&&v.sessionId===this.session&&this.loader){
          const f=v.params?.frame;
          if(!f||f.parentId===undefined&&f.loaderId!==this.loader)this.reject('DOCUMENT_CHANGED');
        }
      }
    });
    child.on('exit',()=>this.reject());child.on('error',()=>this.reject());
    child.stdio[4].on('error',()=>this.reject());child.stdio[3].on('error',()=>this.reject());
  }
  reject(code='CDP_LOST'){this.failed=true;for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(failure(code,{method:p.method}));}this.pending.clear();}
  call(method,params={},sessionId){
    if(this.failed)return Promise.reject(failure('CDP_LOST',{method}));
    const id=++this.id;
    return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>{this.pending.delete(id);this.reject('CDP_TIMEOUT');reject(failure('CDP_TIMEOUT',{method}));},2000);
      this.pending.set(id,{resolve,reject,timer,method});
      this.child.stdio[3].write(encode({id,method,params,...(sessionId?{sessionId}:{})})+'\0');
    });
  }
  async evaluate(expression){
    const r=await this.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},this.session);
    if(r?.exceptionDetails||!r?.result||r.result.type==='error')throw failure('CDP_EVALUATION',{method:'Runtime.evaluate'});
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
  fs.mkdirSync(path.join(root,'screenshots'),{mode:0o700});
  let index=0,screenshots=0,child,cdp,life,stopped=false;const seen=new Set();
  const emit=(phase,data={})=>save(root,String(index++).padStart(5,'0')+'.json',{
    schema:'s3-local-browser-lifetime/1',phase,pid:process.pid,parent_pid:+parentText,
    browser_pid:child?.pid??null,monotonic_ns:process.hrtime.bigint().toString(),...data,
    keys_exported:false,private_key_storage:false,cleanup_complete_verified:false});
  const diagnostic=new Diagnostic(emit),started=clock();
  const guard=()=>{if(stopped)throw failure('STOPPED');if(process.ppid!==+parentText)throw failure('PARENT_LOST');if(clock()-started>=+lifetimeText*1000)throw failure('DEADLINE');};
  const step=(phase,fn)=>diagnostic.step(phase,async()=>{guard();return fn();});
  process.on('SIGINT',()=>{stopped=true;});process.on('SIGTERM',()=>{stopped=true;});
  try{
    diagnostic.current='chrome_spawn';
    child=spawn(chrome,['--headless=new','--remote-debugging-pipe','--no-first-run','--no-default-browser-check',
      '--disable-background-networking','--disable-sync','--disable-breakpad','--disable-crash-reporter',
      '--password-store=basic','--use-mock-keychain','--user-data-dir='+profile,'about:blank'],{
      stdio:['ignore','ignore','ignore','pipe','pipe'],env:{LANG:'C',LC_ALL:'C',PATH:'/usr/bin:/bin'}});
    cdp=new CDP(child);emit('spawned');diagnostic.completed='chrome_spawn';
    cdp.target=await step('target_create',async()=>{
      const target=id((await cdp.call('Target.createTarget',{url:'http://127.0.0.1:5173/'}))?.targetId);
      if(!target)throw failure('TARGET_ID');return target;
    });
    cdp.session=await step('target_attach',async()=>{
      const session=id((await cdp.call('Target.attachToTarget',{targetId:cdp.target,flatten:true}))?.sessionId);
      if(!session)throw failure('SESSION_ID');return session;
    });
    await step('page_enable',()=>cdp.call('Page.enable',{},cdp.session));
    await step('viewport',()=>cdp.call('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false},cdp.session));
    // Bounded readiness, before any keys. The parent has already bound the
    // exact approved static assets; no board wait is inside this process.
    const readyUntil=clock()+10000;
    while(!await step('page_readiness',()=>cdp.evaluate('Boolean(document.getElementById("prepare"))'))){
      if(clock()>=readyUntil)throw failure('PAGE_NOT_READY');await new Promise(r=>setTimeout(r,100));
    }
    await step('frame_identity',async()=>{
      const frame=(await cdp.call('Page.getFrameTree',{},cdp.session))?.frameTree?.frame;
      cdp.loader=id(frame?.loaderId);cdp.frame=id(frame?.id);
      if(!cdp.loader||!cdp.frame)throw failure('FRAME_ID');
    });
    const request=(action,a)=>step('ui_'+action,async()=>{
      if(cdp.failed)throw failure('CDP_LOST');
      if(action==='observe')return cdp.evaluate(`({nonce:${encode(a.challenge)},document:globalThis.__nus_lifetime_document,origin:location.origin,closed:false,registration:document.getElementById('registration').textContent})`);
      if(action==='prepare')return cdp.evaluate(`(()=>{document.getElementById('enabled').checked=true;document.getElementById('acknowledge').checked=true;document.getElementById('prepare').click();return {registration:document.getElementById('registration').textContent};})()`);
      if(action==='context'){
        const response=await cdp.evaluate(`(async()=>{const controller=new AbortController();const timer=setTimeout(()=>controller.abort(),1500);try{const r=await fetch('/runtime-context.json',{signal:controller.signal,cache:'no-store',credentials:'omit',redirect:'error'});if(!r.ok||r.headers.get('content-type')!=='application/json')return {status:r.status,ok:false};const reader=r.body.getReader();let size=0,chunks=[];try{for(;;){const v=await reader.read();if(v.done)break;size+=v.value.length;if(size>16384)throw Error();chunks.push(v.value);}}finally{await reader.cancel();}const bytes=new Uint8Array(size);let offset=0;for(const v of chunks){bytes.set(v,offset);offset+=v.length;}return {status:r.status,ok:true,context:JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes))};}finally{clearTimeout(timer);}})()`);
        if(response?.ok!==true)throw failure('CONTEXT_HTTP',{http_status:Number.isInteger(response?.status)?response.status:null});
        emit('http',{method:'GET',route:'runtime-context',http_status:response.status});return response.context;
      }
      if(action==='activate'){
        await cdp.evaluate(`document.getElementById('activate').click()`);
        const until=clock()+3000;
        while(!await cdp.evaluate(`document.getElementById('wallet').querySelectorAll('button').length>0`)){
          guard();if(clock()>=until)throw failure('ACTIVATION');await new Promise(r=>setTimeout(r,50));
        }
        return;
      }
      if(['click','select','amount','input','snapshot'].includes(action))return cdp.evaluate(uiExpression(action,a));
      if(action==='screenshot'){
        if(screenshots>=16)throw failure('SCREENSHOT_LIMIT');
        screenshots++;
        const r=await cdp.call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false,clip:{x:0,y:0,width:1280,height:900,scale:1}},cdp.session);
        if(typeof r?.data!=='string'||r.data.length>1398104)throw failure('SCREENSHOT_SIZE');
        const bytes=Buffer.from(r.data,'base64');
        if(bytes.toString('base64')!==r.data||bytes.length>1048576||!bytes.subarray(0,8).equals(Buffer.from('89504e470d0a1a0a','hex')))throw failure('SCREENSHOT_FORMAT');
        const name=nonce()+'.png',fd=fs.openSync(path.join(root,'screenshots',name),'wx',0o600);
        try{fs.writeFileSync(fd,bytes);fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
        const dir=fs.openSync(path.join(root,'screenshots'),'r');try{fs.fsyncSync(dir);}finally{fs.closeSync(dir);}
        return {file:'screenshots/'+name,bytes:bytes.length,sha256:hash(bytes),width:1280,height:900,public_ui_only:true};
      }
      throw failure('ACTION');
    });
    life=new Lifetime({request,parent:()=>process.ppid===+parentText&&!cdp.failed&&!stopped,clock,limit:Math.max(1,+lifetimeText*1000-(clock()-started)),emit});
    await step('document_nonce',()=>cdp.evaluate(`Object.defineProperty(globalThis,'__nus_lifetime_document',{value:${encode(life.document)},writable:false,configurable:false})`));
    emit('tab_ready',{target_id:cdp.target,session_id:cdp.session,frame_id:cdp.frame,loader_id:cdp.loader,document_nonce:life.document});
    while(!stopped&&life.state!=='CLOSED'){
      await life.observe();
      diagnostic.current='command_read';
      const names=fs.readdirSync(path.join(root,'commands')).sort();if(names.length>1000)throw failure('COMMAND_LIMIT');
      for(const name of names){
        if(/^\.[0-9a-f]{64}\.json\.pending$/.test(name))continue;
        if(seen.has(name))continue;
        if(!/^[0-9a-f]{64}\.json$/.test(name))throw failure('COMMAND');
        const file=path.join(root,'commands',name),s=fs.lstatSync(file);
        if(!s.isFile()||s.isSymbolicLink()||s.nlink!==1||s.uid!==process.getuid()||(s.mode&0o777)!==0o600||s.size>65536)throw failure('COMMAND');
        const cmd=JSON.parse(fs.readFileSync(file,'utf8'));seen.add(name);
        if(cmd.nonce!==name.slice(0,-5)||!['prepare','inspect','activate','click','select','amount','input','snapshot','screenshot','finish'].includes(cmd.action))throw failure('COMMAND');
        const fields={click:['name'],select:['name','value'],amount:['value'],input:['name','value']}[cmd.action]??[];
        if(Object.keys(cmd).sort().join(',')!==['nonce','action',...fields].sort().join(','))throw failure('COMMAND');
        if(cmd.action==='activate'){
          const contextFile=path.join(root,'active-context.json');
          const cs=fs.lstatSync(contextFile);if(!cs.isFile()||cs.isSymbolicLink()||cs.nlink!==1||cs.size>16384||(cs.mode&0o777)!==0o600)throw failure('CONTEXT');
          cmd.context=JSON.parse(fs.readFileSync(contextFile,'utf8'));
          cmd.registration=await cdp.evaluate("document.getElementById('registration').textContent");
        }
        const result=await life.act(cmd.action,cmd);
        if(cmd.action==='prepare')save(root,'registration.json',JSON.parse(await cdp.evaluate("document.getElementById('registration').textContent")));
        save(path.join(root,'results'),name,{nonce:cmd.nonce,action:cmd.action,state:life.state,result:result??null});
      }
      await new Promise(r=>setTimeout(r,500));
    }
    if(stopped)throw failure('STOPPED');
  }catch(error){diagnostic.fail(error,{target_id:cdp?.target??null,session_id:cdp?.session??null,frame_id:cdp?.frame??null,loader_id:cdp?.loader??null});process.exitCode=2;}
  finally{
    if(child){
      let closeError=null;
      try{if(!cdp.failed)await cdp.call('Browser.close');}catch(e){closeError=e.code??'CLOSE_FAILED';}
      if(child.exitCode===null&&child.signalCode===null){child.kill('SIGTERM');await new Promise(r=>setTimeout(r,500));}
      if(child.exitCode===null&&child.signalCode===null)child.kill('SIGKILL');
      if(child.exitCode===null&&child.signalCode===null)await new Promise(r=>child.once('exit',r));
      emit('closed',{browser_exit:child.exitCode,browser_signal:child.signalCode,close_error:closeError,keys_discarded:true});
    }
  }
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  main(process.argv.slice(2)).catch(()=>{process.stderr.write('LOCAL_BROWSER_REJECTED\n');process.exitCode=2;});
}
