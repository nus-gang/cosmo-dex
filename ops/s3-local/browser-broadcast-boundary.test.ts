import {writeFileSync,readFileSync,mkdtempSync,rmSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {join} from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import fixture from '../../web/s3/fixtures/ld-rest.json' with {type:'json'};
import {LocalClient, type Entry} from '../../web/s3/client.ts';
import {LocalKey} from '../../web/s3/key.ts';
import {base64} from '../../web/s3/direct-codec.ts';
import {encode} from '../../web/src/codec.ts';
import {PREFIX} from '../../web/s3/state.ts';

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
    if(path==='capabilities')return json({...fixture.receipt,api_prefix:PREFIX,signed_result_query:true,automatic_withdraw:false,ws:false});
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

test('approved Wallet emits exact authenticated broadcast body for Rust adapter',async()=>{
 const f=harness();try{
  await f.login();const entry=await f.c.withdraw('DEVBASE','1');
  const request=f.seen.find(r=>r.path==='chain/broadcast')!;
  assert.deepEqual(JSON.parse(String(request.init.body)),{tx_bytes:entry.tx_bytes});
  assert.equal(entry.state,'SUBMISSION_UNKNOWN');assert.equal(f.broadcasts(),1);
  assert.equal(new Headers(request.init.headers).get('Authorization'),'Bearer synthetic-session-0');
  const output=process.env.NUS_BROWSER_BOUNDARY;
  assert.ok(output);
  writeFileSync(output,JSON.stringify({body:String(request.init.body),account:f.direct(),
    context:ctx,owner:f.keys[0].owner,tx_hash:entry.tx_hash}),{mode:0o600,flag:'wx'});
 }finally{f.clean();}
});

// Real Rust adapter subprocess; authentication, Account/proof and verifier are
// injected. No socket, RPC, Go signature verification or service startup.
test('Wallet three routes consume actual Rust responses and retain UNKNOWN without another TX', {skip:!process.env.NUS_ROUTE_BINARY},async()=>{
 const f=harness();const dir=mkdtempSync(join(process.env.PAPERCLIP_RUN_SCRATCH_DIR!,'routes-'));
 let calls=0,sends=0,entry:Entry|undefined,queryError=true;
 try {
  for(const route of ['chain/account','chain/broadcast','chain/result']) f.handlers.set(route,async()=>{
   const request=f.seen.at(-1)!;
   const input=join(dir,`in-${calls}.json`),output=join(dir,`out-${calls++}.json`);
   writeFileSync(input,JSON.stringify({context:ctx,owner:f.keys[0].owner,account:f.direct(),
    method:request.init.method??'GET',path:PREFIX+route,body:String(request.init.body??''),
    authorization:new Headers(request.init.headers).get('Authorization'),query_error:queryError,
    result:entry?{context:ctx,tx_hash:entry.tx_hash,tx_bytes:entry.tx_bytes,height:'101',code:'0',state:'COMMITTED'}:{}}));
   execFileSync(process.env.NUS_ROUTE_BINARY!,['wallet_three_route_bridge','--ignored','--test-threads=1'],
    {env:{...process.env,NUS_ROUTE_INPUT:input,NUS_ROUTE_OUTPUT:output},timeout:10000});
   const r=JSON.parse(readFileSync(output,'utf8'));sends+=r.sends;
   return new Response(JSON.stringify(r.body),{status:r.status});
  });
  await f.login();entry=await f.c.withdraw('DEVBASE','1');
  assert.equal(entry.state,'SUBMISSION_UNKNOWN');assert.equal(sends,1);
  await f.c.resolve(entry);assert.equal(entry.state,'SUBMISSION_UNKNOWN');
  await assert.rejects(f.c.withdraw('DEVBASE','1'));
  assert.equal(f.signatures(),1);assert.equal(f.broadcasts(),1);assert.equal(sends,1);
  queryError=false;await f.c.resolve(entry);assert.equal(entry.state,'COMMITTED');
  assert.equal(entry.height,'101');assert.equal(f.signatures(),1);assert.equal(sends,1);
  assert.equal(f.seen.filter(r=>r.path==='chain/account').length,1);
  assert.equal(f.seen.filter(r=>r.path==='chain/result').length,2);
 }finally{f.clean();rmSync(dir,{recursive:true,force:true});}
});
