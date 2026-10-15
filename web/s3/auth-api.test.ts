import test from 'node:test';
import assert from 'node:assert/strict';
import fixture from './fixtures/ld-rest.json' with {type:'json'};
import {LocalClient, type Entry} from './client.ts';
import {LocalKey} from './key.ts';
import {base64} from './direct-codec.ts';
import {encode} from '../src/codec.ts';
import {PREFIX,PUBLIC_RECEIPT_SCHEMA_SHA256,PUBLIC_RECEIPT_VERSION,TRUSTED_RECEIPT_VERSION} from './state.ts';

const ctx=fixture.owner_projection.context,origin='http://127.0.0.1:5173';
const json=(value:unknown)=>new Response(JSON.stringify(value));
function deferred<T>() {let resolve!:(v:T)=>void;const promise=new Promise<T>(r=>resolve=r);return {promise,resolve};}
function harness(flags:[boolean,boolean]=[true,true]) {
  const keys=[new LocalKey(),new LocalKey()];
  let selected=0,signatures=0;
  for(const key of keys){const direct=key.direct.bind(key);key.direct=(input)=>{signatures++;return direct(input);};}
  const seen:{path:string;init:RequestInit}[]=[];
  const handlers=new Map<string,()=>Promise<Response>>();
  const view=()=>({...structuredClone(fixture.other_projection),owner:keys[selected].owner,received_at_unix_ms:String(Date.now()),withdraw_frozen:true,withdraw_ready:true});
  const direct=()=>({context:ctx,owner:keys[selected].address,public_key_base64:base64.encode(keys[selected].publicKey),account_number:'1',sequence:'0',owner_epoch:'0',observed_height:'100',received_at_unix_ms:String(Date.now()),gas_atoms:'1000'});
  const transport:typeof fetch=async(input,init={})=>{
    const path=String(input).slice(PREFIX.length);
    assert.equal(String(input),PREFIX+path);
    assert.ok(['auth/challenge','auth/session','capabilities','account','chain/account','chain/broadcast','chain/result'].includes(path));
    assert.equal(init.cache,'no-store');assert.equal(init.redirect,'error');assert.ok(init.signal);
    seen.push({path,init});
    if(!path.startsWith('auth/'))assert.equal(new Headers(init.headers).get('Authorization'),'Bearer synthetic-session-'+selected);
    if(handlers.has(path))return handlers.get(path)!();
    if(path==='auth/challenge'){
      const now=Math.floor(Date.now()/1000);
      return json({wire_base64:base64.encode(encode('WalletChallengeV1',{protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,owner:keys[selected].owner,server_origin:origin,audience:'exchange-api',challenge_nonce:'aa'.repeat(32),issued_at:String(now),expiry_time:String(now+100)}))});
    }
    if(path==='auth/session')return json({token:'synthetic-session-'+selected});
    if(path==='capabilities')return json({...fixture.receipt,api_prefix:PREFIX,public_receipt_version:PUBLIC_RECEIPT_VERSION,public_receipt_schema_sha256:PUBLIC_RECEIPT_SCHEMA_SHA256,trusted_receipt_version:TRUSTED_RECEIPT_VERSION,signed_result_query:true,automatic_withdraw:false,ws:false});
    if(path==='account')return json(view());
    if(path==='chain/account')return json(direct());
    if(path==='chain/broadcast')return json({state:'UNKNOWN'});
    return new Response('{}',{status:404});
  };
  const c=LocalClient.authenticated(ctx,transport,...flags);
  c.select(keys[0]);
  return {c,keys,seen,handlers,view,direct,signatures:()=>signatures,
    broadcasts:()=>seen.filter(r=>r.path==='chain/broadcast').length,
    switch:()=>{selected=1;c.select(keys[1]);},
    login:()=>c.login(origin),clean:()=>{c.destroy();for(const k of keys)k.destroy();}};
}
const terminal=(entry:Entry)=>({context:ctx,tx_bytes:entry.tx_bytes,tx_hash:entry.tx_hash,height:'101',code:'0',state:'COMMITTED'});

test('fixed adapter uses private session on all three routes; UNKNOWN blocks duplicate TX',async()=>{
  const f=harness();try{
    await f.login();const entry=await f.c.withdraw('DEVBASE','1');
    assert.equal(entry.state,'SUBMISSION_UNKNOWN');assert.equal(f.signatures(),1);assert.equal(f.broadcasts(),1);
    const account=f.seen.find(r=>r.path==='chain/account')!,broadcast=f.seen.find(r=>r.path==='chain/broadcast')!;
    assert.equal(account.init.method,'GET');assert.equal(account.init.body,undefined);
    assert.equal(broadcast.init.method,'POST');assert.deepEqual(JSON.parse(String(broadcast.init.body)),{tx_bytes:entry.tx_bytes});
    assert.equal(base64.encode(base64.decode(entry.tx_bytes)),entry.tx_bytes);
    await f.c.resolve(entry);await f.c.refresh();await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/HELD/);
    assert.equal(entry.state,'SUBMISSION_UNKNOWN');assert.equal(f.broadcasts(),1);
    f.handlers.set('chain/result',async()=>json(terminal(entry)));
    await f.c.resolve(entry);assert.equal(entry.state,'COMMITTED');
    const result=f.seen.find(r=>r.path==='chain/result')!;
    assert.equal(result.init.method,'POST');assert.deepEqual(JSON.parse(String(result.init.body)),{tx_hash:entry.tx_hash});
    assert.equal(f.c.canWithdraw(),false);assert.equal(f.broadcasts(),1);
    assert.equal('chain' in f.c,false);assert.equal(JSON.stringify(f.c).includes('synthetic-session'),false);
  }finally{f.clean();}
});
for(const flags of [[false,false],[true,false],[false,true]] as [boolean,boolean][])test('adapter opt-ins '+flags+': IO0',async()=>{
 const f=harness(flags);try{await assert.rejects(f.login,/OPT_INS/);assert.equal(f.seen.length,0);await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/HELD/);assert.equal(f.broadcasts(),0);}finally{f.clean();}
});
test('unapproved origin is rejected before IO',async()=>{
 const f=harness();try{await assert.rejects(()=>f.c.login('https://example.com'),/ORIGIN/);assert.equal(f.seen.length,0);}finally{f.clean();}
});
for(const action of ['switch','destroy','revoke','hold'] as const)test('pending direct account then '+action+': signature0/broadcast0',async()=>{
 const f=harness();try{
  await f.login();const pending=deferred<Response>(),reply=f.direct();f.handlers.set('chain/account',()=>pending.promise);
  const withdrawal=f.c.withdraw('DEVBASE','1');
  if(action==='switch')f.switch();else if(action==='destroy')f.c.destroy();else if(action==='revoke')f.c.revokeSession();else f.c.projection.close('DISCONNECTED');
  pending.resolve(json(reply));await assert.rejects(()=>withdrawal);
  assert.equal(f.signatures(),0);assert.equal(f.broadcasts(),0);assert.equal(f.c.history.length,0);
  assert.equal(f.c.canWithdraw(),false);
  if(action!=='hold')assert.equal(f.seen.find(r=>r.path==='chain/account')!.init.signal!.aborted,true);
 }finally{f.clean();}
});
for(const code of [401,403,503])test('direct account HTTP '+code+' closes signing and broadcast',async()=>{
 const f=harness();try{await f.login();f.handlers.set('chain/account',async()=>new Response('{}',{status:code}));
 await assert.rejects(()=>f.c.withdraw('DEVBASE','1'));assert.equal(f.signatures(),0);assert.equal(f.broadcasts(),0);assert.equal(f.c.canWithdraw(),false);
 if(code!==503){await assert.rejects(()=>f.c.refresh(),/CAPABILITY/);}
 }finally{f.clean();}
});
test('401 on concurrent account revokes delayed direct account and requires login',async()=>{
 const f=harness();try{
  await f.login();const pending=deferred<Response>(),reply=f.direct();f.handlers.set('chain/account',()=>pending.promise);
  const withdrawal=f.c.withdraw('DEVBASE','1');f.handlers.set('account',async()=>new Response('{}',{status:401}));assert.equal(await f.c.refresh(),false);
  pending.resolve(json(reply));await assert.rejects(()=>withdrawal);assert.equal(f.signatures(),0);assert.equal(f.broadcasts(),0);
  await assert.rejects(()=>f.c.refresh(),/CAPABILITY/);
  f.handlers.delete('account');f.handlers.delete('chain/account');await f.login();assert.equal(f.c.canWithdraw(),true);
 }finally{f.clean();}
});
test('stale direct observation is not restamped by HTTP bridge',async()=>{
 const f=harness();try{await f.login();f.handlers.set('chain/account',async()=>json({...f.direct(),received_at_unix_ms:String(Date.now()-3000)}));
 await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/STALE/);assert.equal(f.signatures(),0);assert.equal(f.broadcasts(),0);}finally{f.clean();}
});
test('two second deadline rejects late body even when injected transport ignores abort',async()=>{
 const f=harness();try{await f.login();const pending=deferred<unknown>();f.handlers.set('chain/account',async()=>({ok:true,json:()=>pending.promise}) as Response);
 const withdrawal=f.c.withdraw('DEVBASE','1');await new Promise(r=>setTimeout(r,2050));pending.resolve(f.direct());
 await assert.rejects(()=>withdrawal,/EXPIRED/);assert.equal(f.signatures(),0);assert.equal(f.broadcasts(),0);assert.equal(f.c.canWithdraw(),false);
 }finally{f.clean();}
});
for(const action of ['switch','revoke','destroy'] as const)test('delayed terminal result after '+action+' preserves UNKNOWN',async()=>{
 const f=harness();try{await f.login();const entry=await f.c.withdraw('DEVBASE','1'),pending=deferred<Response>();
 f.handlers.set('chain/result',()=>pending.promise);const result=f.c.resolve(entry);
 if(action==='switch')f.switch();else if(action==='revoke')f.c.revokeSession();else f.c.destroy();
 pending.resolve(json(terminal(entry)));await result;assert.equal(entry.state,'SUBMISSION_UNKNOWN');assert.equal(f.broadcasts(),1);
 const before=f.seen.length;await f.c.resolve(entry);assert.equal(f.seen.length,before);
 }finally{f.clean();}
});
test('delayed old session cannot replace new account session',async()=>{
 const f=harness();try{
  const session=deferred<Response>(),reached=deferred<void>();f.handlers.set('auth/session',()=>{reached.resolve();return session.promise;});
  const old=f.login();await reached.promise;f.switch();f.handlers.delete('auth/session');await f.login();
  session.resolve(json({token:'obsolete-session'}));await assert.rejects(()=>old,/ACCOUNT_CHANGED/);
  await f.c.withdraw('DEVBASE','1');assert.equal(f.broadcasts(),1);
 }finally{f.clean();}
});
test('old 401 cannot revoke the new session',async()=>{
 const f=harness();try{
  await f.login();const pending=deferred<Response>();f.handlers.set('chain/account',()=>pending.promise);const old=f.c.withdraw('DEVBASE','1');
  f.switch();f.handlers.delete('chain/account');await f.login();pending.resolve(new Response('{}',{status:401}));await assert.rejects(()=>old,/ACCOUNT_CHANGED/);
  assert.equal(f.c.canWithdraw(),true);assert.equal(f.signatures(),0);await f.c.withdraw('DEVBASE','1');assert.equal(f.broadcasts(),1);
 }finally{f.clean();}
});
test('lost broadcast response stays UNKNOWN across revoke/login; only query is allowed',async()=>{
 const f=harness();try{
  await f.login();f.handlers.set('chain/broadcast',async()=>{throw Error('lost response');});const entry=await f.c.withdraw('DEVBASE','1');
  f.c.revokeSession();await f.login();await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/HELD/);await f.c.resolve(entry);
  assert.equal(entry.state,'SUBMISSION_UNKNOWN');assert.equal(f.signatures(),1);assert.equal(f.broadcasts(),1);
 }finally{f.clean();}
});
