import test from 'node:test';
import assert from 'node:assert/strict';
import fixture from './fixtures/ld-rest.json' with {type:'json'};
import { Projection, capability, envelope, PREFIX, type Account } from './state.ts';
import { LocalClient, type ChainPort } from './client.ts';
import { LocalKey } from './key.ts';
import { screen } from './component.ts';
import { ml_dsa65 } from '../src/wallet.ts';
import { envelope as txEnvelope } from './direct-codec.ts';
import { base64, integer } from './direct-codec.ts';
import { encode } from '../src/codec.ts';
const ctx=fixture.owner_projection.context;
const caps={...fixture.receipt,api_prefix:PREFIX,signed_result_query:true,automatic_withdraw:false,ws:false};
const account=(owner='alice',now=Date.now()): Account=>({...structuredClone(fixture.other_projection),durable_ack:false,owner,received_at_unix_ms:String(now),withdraw_frozen:true,withdraw_ready:true});
function setup() {const p=new Projection(ctx);p.select('alice');return p;}
test('approved L-D raw projection and receipt remain developer evidence, not assets',()=>{
  const p=new Projection(ctx);p.select(fixture.owner_projection.owner);
  assert.equal(p.accept(fixture.owner_projection as Account,p.generation,Number(fixture.owner_projection.received_at_unix_ms),0),true);
  assert.equal(p.ready(),false);envelope(fixture.receipt,ctx);capability(caps,ctx);
  assert.equal(p.view!.ledger[0].R,'1000000');
});
test('capability rejects profile/context/guarantee/fallback drift',()=>{
  for(const change of [{durable_ack:true},{api_prefix:'/s2/'},{automatic_withdraw:true},{ws:true},{signed_result_query:false},{profile_id:'standard'},{context:{...ctx,genesis_hash:'00'.repeat(32)}},{development_receipt:'COMMITTED'}])assert.throws(()=>capability({...caps,...change},ctx));
});
test('atomic integer ledger rejects float, negative, excess and P reused in A',()=>{
  for(const change of [{C:'1.0'},{A:'-1'},{A:'1000000000001',P:'1'},{C:(1n<<128n).toString()},{R:'1000000000001'}]){
    const p=setup(),v=account();Object.assign(v.ledger[0],change);assert.equal(p.accept(v,p.generation,Date.now(),0),false);assert.equal(p.view,undefined);
  }
});
test('same revision idempotence, conflict, gap requery and regression close admission',()=>{
  const p=setup(),now=Date.now(),v=account('alice',now),g=p.generation;
  assert.ok(p.accept(v,g,now,0));assert.ok(p.accept(v,g,now,0));assert.equal(p.view!.ledger[0].C,v.ledger[0].C);
  assert.equal(p.accept({...v,revision:'3'},g,now,0),false);assert.equal(p.ready(now),false);
  assert.ok(p.accept({...v,revision:'3'},g,now,0));assert.equal(p.accept(v,g,now,0),false);assert.equal(p.ready(now),false);
  const q=setup();q.accept(v,q.generation,now,0);const changed=structuredClone(v);changed.ledger[0].C='9';changed.ledger[0].A='9';assert.equal(q.accept(changed,q.generation,now,0),false);
});
test('stale, delayed, disconnected, recovery and backward clock keep withdrawal closed',()=>{
  const now=Date.now();
  for(const [change,elapsed] of [[{received_at_unix_ms:String(now-6000)},0],[{},2001],[{fresh:false},0],[{gate:'RECOVERY_REQUIRED'},0],[{indexer_height:'99'},0]] as const){const p=setup();p.accept({...account('alice',now),...change},p.generation,now,elapsed);assert.equal(p.ready(now),false);}
  let mono=0;const p=new Projection(ctx,()=>mono);p.select('alice');p.accept(account('alice',now),p.generation,now,0);mono=5001;assert.equal(p.ready(now),false);p.close('DISCONNECTED');assert.equal(p.ready(now),false);assert.equal(p.open(now-1),false);
});
test('account switch erases old view and ignores delayed foreign response',()=>{
  const p=setup(),g=p.generation;assert.ok(p.accept(account(),g,Date.now(),0));p.select('bob');assert.equal(p.view,undefined);assert.equal(p.accept(account(),g,Date.now(),0),false);assert.equal(p.view,undefined);assert.equal(p.accept(account(),p.generation,Date.now(),0),false);
});
test('D/P/R held, COMMITTED requires receipt, committed cannot be corrected',()=>{
  for(const field of ['R','D','P'] as const){const p=setup(),v=account();v.ledger[0][field]='1';if(field!=='P')v.ledger[0].A='999999999999';assert.ok(p.accept(v,p.generation,Date.now(),0));assert.equal(p.ready(),false);}
  const p=setup(),v=account();v.fills=[{fill_id:'f',state:'COMMITTED',revision:'1',batch:{batch_id:'ab'.repeat(32)}}];assert.equal(p.accept(v,p.generation,Date.now(),0),false);
  v.batches=[{state:'COMMITTED',revision:'1',batch:{batch_id:'ab'.repeat(32),batch_hash:'cd'.repeat(32),batch_seq:'1'},receipt:{disposition:'COMMITTED',terminal_height:'100',terminal_tx_hash:'ef'.repeat(32),batch_receipt_v2:'Zg=='}}];assert.ok(p.accept(v,p.generation,Date.now(),0));v.revision='2';v.fills[0].state='CORRECTED';assert.equal(p.accept(v,p.generation,Date.now(),0),false);
});
async function clientFixture() {
  const key=new LocalKey();let owner=key.owner,posts=0,prepare=0,view=account(owner);let directWait: Promise<void>|undefined;let accountReply: (()=>Promise<Response>)|undefined;
  const chain:ChainPort={account:async()=>{if(directWait)await directWait;return {context:ctx,owner:key.address,public_key_base64:base64.encode(key.publicKey),account_number:'1',sequence:'0',owner_epoch:'0',observed_height:'100',received_at_unix_ms:String(Date.now()),gas_atoms:'1000'};},broadcast:async()=>{posts++;throw Error('response lost');},result:async()=>{throw Error('NOT_FOUND');}};
  const transport:typeof fetch=async(path,init)=>{
    const route=String(path).slice(PREFIX.length);assert.ok(String(path).startsWith(PREFIX));
    let value:unknown;
    if(route==='auth/challenge') {const now=Math.floor(Date.now()/1000);value={wire_base64:base64.encode(encode('WalletChallengeV1',{protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,owner,server_origin:'http://127.0.0.1:5173',audience:'exchange-api',challenge_nonce:'aa'.repeat(32),issued_at:String(now),expiry_time:String(now+100)}))};}
    else if(route==='auth/session')value={token:'fixture-session'};
    else {assert.equal((init?.headers as any).Authorization,'Bearer fixture-session');if(route==='capabilities')value=caps;else if(route==='account'){if(accountReply)return accountReply();value=view;}else{prepare++;value=fixture.receipt;}}
    return new Response(JSON.stringify(value));
  };
  const c=new LocalClient(ctx,transport,chain,true,true);c.select(key);await c.login('http://127.0.0.1:5173');
  return {c,key,chain,posts:()=>posts,prepare:()=>prepare,setWait:(p:Promise<void>)=>{directWait=p;},setView:(v:Account)=>{view=v;},setAccountReply:(reply:()=>Promise<Response>)=>{accountReply=reply;}};
}
test('default disabled and missing second opt-in perform zero network requests',async()=>{
  for(const flags of [[false,false],[true,false],[false,true]]){let requests=0;const key=new LocalKey();const c=new LocalClient(ctx,async()=>{requests++;throw Error();},undefined,...flags as [boolean,boolean]);c.select(key);await assert.rejects(()=>c.login('http://127.0.0.1:5173'),/OPT_INS/);assert.equal(requests,0);c.destroy();}
});
test('direct user ML-DSA signing, duplicate click, lost response, NOT_FOUND: one TX only',async()=>{
  const f=await clientFixture();try {
    let release!:()=>void;f.setWait(new Promise(r=>release=r));const first=f.c.withdraw('DEVBASE','1');await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/HELD/);release();const entry=(await first)!;
    assert.equal(f.posts(),1);assert.equal(entry.state,'SUBMISSION_UNKNOWN');assert.ok(base64.decode(entry.tx_bytes).length>5000);assert.ok(ml_dsa65.verify(f.key.publicKey,txEnvelope(entry.input,f.key.publicKey).signDoc,base64.decode(entry.tx_bytes).slice(-3309)));
    await f.c.resolve(entry);await f.c.refresh();assert.equal(f.c.canWithdraw(),false);await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/HELD/);
    f.chain.result=async()=>({context:ctx,tx_bytes:entry.tx_bytes,tx_hash:entry.tx_hash,height:'101',code:'0',state:'COMMITTED'});
    await f.c.resolve(entry);assert.equal(entry.state,'COMMITTED');assert.equal(f.c.canWithdraw(),false);assert.equal(f.posts(),1);
  }finally{f.c.destroy();}
});
test('switch during direct account response generates no signature or broadcast',async()=>{
  const f=await clientFixture();try{let release!:()=>void;f.setWait(new Promise(r=>release=r));const pending=f.c.withdraw('DEVBASE','1');f.c.select();release();await assert.rejects(()=>pending,/ACCOUNT_CHANGED/);assert.equal(f.posts(),0);assert.equal(f.c.history.length,0);assert.equal(screen(f.c).ledger.length,0);}finally{f.key.destroy();}
});
test('prepare developer receipt cannot mutate C; view displays guarantee, hold and account boundary',async()=>{
  const f=await clientFixture();try{const before=f.c.projection.view!.ledger[0].C;await f.c.prepare();assert.equal(f.prepare(),1);assert.equal(f.c.projection.view!.ledger[0].C,before);assert.match(screen(f.c).receipt,/체인 확정 아님/);assert.match(screen(f.c).notice,/durable_ack=false/);f.c.select();assert.deepEqual(screen(f.c).history,[]);assert.deepEqual(screen(f.c).ledger,[]);}finally{f.key.destroy();}
});
test('noncanonical amount rejected before signing or broadcast',async()=>{
 const f=await clientFixture();try{for(const amount of ['1.1','-1','01','0',(1n<<128n).toString()])await assert.rejects(()=>f.c.withdraw('DEVBASE',amount));assert.equal(f.posts(),0);assert.equal(f.c.history.length,0);}finally{f.c.destroy();}
});
test('pending, unknown and corrected are distinct labels; P never enters C',()=>{
 const c=new LocalClient(ctx,fetch);c.projection.select('alice');const now=Date.now();
 for(const [i,state] of ['PENDING','SUBMISSION_UNKNOWN','CORRECTED'].entries()){
  const v=account('alice',now);v.revision=String(i+1);v.ledger[0].P=state==='CORRECTED'?'0':'1000000';v.withdraw_ready=false;v.fills=[{fill_id:'f',state,revision:String(i+1)}];
  assert.ok(c.projection.accept(v,c.projection.generation,now,0));assert.match(screen(c).fills[0],new RegExp(['잠정','제출 결과 불명','정정'][i]));assert.equal(c.projection.view!.ledger[0].C,'1000000000000');assert.equal(screen(c).disabled,true);
 }
});

function deferred<T>() {let resolve!:(value:T)=>void;let reject!:(error:Error)=>void;const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
for(const fault of ['recovery','503','disconnect','explicit-close'] as const) {
  test(`delayed OPEN after ${fault} cannot reopen or sign/broadcast; fresh post-hold query can recover`,async()=>{
    const f=await clientFixture();let signatures=0;const direct=f.key.direct.bind(f.key);f.key.direct=(input)=>{signatures++;return direct(input);};
    try {
      const old=deferred<Response>(),latest=deferred<Response>();let requests=0;
      const open=structuredClone(f.c.projection.view!);
      f.setAccountReply(()=>++requests===1?old.promise:latest.promise);
      const a=f.c.refresh();
      if(fault==='explicit-close')f.c.projection.close('DISCONNECTED');
      else {
        const b=f.c.refresh();
        if(fault==='recovery')latest.resolve(new Response(JSON.stringify({...open,gate:'RECOVERY_REQUIRED',withdraw_ready:false})));
        else if(fault==='503')latest.resolve(new Response(null,{status:503}));
        else latest.reject(Error('DISCONNECTED'));
        await b;
      }
      assert.equal(f.c.canWithdraw(),false);
      old.resolve(new Response(JSON.stringify(open)));assert.equal(await a,false);
      assert.equal(f.c.canWithdraw(),false);await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/HELD/);
      assert.equal(signatures,0);assert.equal(f.posts(),0);assert.equal(f.c.history.length,0);
      f.setAccountReply(async()=>new Response(JSON.stringify({...open,received_at_unix_ms:String(Date.now())})));
      assert.equal(await f.c.refresh(),true);assert.equal(f.c.canWithdraw(),true);
      assert.equal(signatures,0);assert.equal(f.posts(),0);
    }finally{f.c.destroy();}
  });
}
for(const fault of ['503','recovery'] as const) test(`query started before ${fault} hold cannot reopen even when it was issued last`,async()=>{
  const f=await clientFixture();try {
    const earlier=deferred<Response>(),later=deferred<Response>();let requests=0;const open=structuredClone(f.c.projection.view!);
    f.setAccountReply(()=>++requests===1?earlier.promise:later.promise);
    const a=f.c.refresh(),b=f.c.refresh();
    earlier.resolve(fault==='503'?new Response(null,{status:503}):new Response(JSON.stringify({...open,gate:'RECOVERY_REQUIRED',withdraw_ready:false})));await a;
    later.resolve(new Response(JSON.stringify(open)));assert.equal(await b,false);
    assert.equal(f.c.canWithdraw(),false);await assert.rejects(()=>f.c.withdraw('DEVBASE','1'),/HELD/);
    assert.equal(f.posts(),0);assert.equal(f.c.history.length,0);
  }finally{f.c.destroy();}
});
test('same revision observation timestamp regression closes admission',()=>{
  const p=setup(),now=Date.now(),v=account('alice',now);
  assert.ok(p.accept({...v,gate:'RECOVERY_REQUIRED',withdraw_ready:false},p.generation,now,0));
  assert.equal(p.accept({...v,received_at_unix_ms:String(now-1)},p.generation,now,0),false);
  assert.equal(p.reason,'OBSERVATION_REGRESSION');assert.equal(p.ready(now),false);
});
test('delayed account refresh after account switch cannot restore prior view',async()=>{
  const f=await clientFixture();try {
    const pending=deferred<Response>(),open=structuredClone(f.c.projection.view!);f.setAccountReply(()=>pending.promise);
    const refresh=f.c.refresh();f.c.select();pending.resolve(new Response(JSON.stringify(open)));
    assert.equal(await refresh,false);assert.equal(f.c.projection.view,undefined);assert.equal(f.c.canWithdraw(),false);
    assert.equal(f.posts(),0);assert.equal(f.c.history.length,0);
  }finally{f.key.destroy();}
});
