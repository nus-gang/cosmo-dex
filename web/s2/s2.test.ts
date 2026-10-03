import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { TradingKey, context, units, base64 } from './session.ts';
import { Views, canonical, type View, type Book } from './state.ts';
import { decode, encode, frame, bytesToHex, hexToBytes } from '../src/codec.ts';
import { ml_dsa65, sha256 } from '../src/wallet.ts';
const ctx = context('ab'.repeat(32));
function view(owner = 'owner', seq = '2'): View {
  return { context: ctx, owner, owner_epoch: '0', stream_seq: seq, revision: seq, snapshot_id: 'cd'.repeat(32), observed_height: '12', ledger: ['DEVBASE','DEVQUOTE'].map(denom => ({denom,C:'100',R:'20',D:'30',P:'70',A:'50'})), orders: [], fills: [], next_cursor: 'END', status: { context: ctx, stream_seq: seq, revision: '10', mode: 'OPEN', reason: 'OK', observation: { snapshot_id: 'cd'.repeat(32), observed_height: '12', fresh: true, last_success_age_ms: '10', block_age_ms: '20', query_latency_ms: '1' }, durability: 'LOCAL_FSYNC', replicated: false, settlement_submission_enabled: false } };
}
function setup() { const s = new Views(ctx, () => 0); s.select('owner'); return s; }
function book(seq = '2'): Book {
  const body = { context: ctx, stream_seq: seq, revision: seq, snapshot_id: 'cd'.repeat(32), observed_height: '12', bids: [], asks: [] };
  return {...body, content_hash: bytesToHex(sha256(frame('NUS/S2/BOOK/V1',new TextEncoder().encode(canonical(body)))))};
}
test('lot/tick uses exact integer decimals and rejects rounding, exponent, overflow', () => {
  assert.equal(units('0.001'),'1'); assert.equal(units('2'),'2000'); assert.equal(units('10'),'10000');
  for (const bad of ['0','0.0001','1e3','01','-1','1000.001','9007199254740993']) assert.throws(() => units(bad));
});
test('Order and Cancel encode actual canonical ML-DSA frames and zeroized session rejects further signing', () => {
  const key = new TradingKey();
  try {
    const o = key.order(ctx,'0','12','SELL','GTC','2','10','01'.repeat(32));
    const raw = base64.decode(o.wire_base64), m = decode('OrderV1',raw);
    assert.equal(m.max_qty_lots,'2000'); assert.equal(m.limit_price_ticks,'10000'); assert.equal(m.expiry_height,'112');
    assert.equal(m.owner,key.owner); assert.equal(m.side,'2'); assert.equal(m.order_type,'1');
    assert(ml_dsa65.verify(key.publicKey,frame('NUS/ORDER/V1',raw),base64.decode(o.signature_base64)));
    const c = key.cancel(ctx,'0','12',m.order_id as string,'02'.repeat(32),'03'.repeat(32));
    assert.equal(decode('CancelV1',base64.decode(c.wire_base64)).order_id,m.order_id);
    assert(ml_dsa65.verify(key.publicKey,frame('NUS/CANCEL/V1',base64.decode(c.wire_base64)),base64.decode(c.signature_base64)));
    assert.throws(() => key.order(ctx,'0','18446744073709551615','BUY','IOC','1','1'));
    key.destroy(); assert.throws(() => key.order(ctx,'0','12','BUY','IOC','1','1'),/SESSION_CLOSED/);
  } finally { key.destroy(); }
});
test('challenge rejects cross owner/genesis/origin/audience and exact expiry before signing', () => {
  const key = new TradingKey();
  try {
    const m = { protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,server_origin:'http://127.0.0.1:5173',audience:'exchange-api',owner:key.owner,challenge_nonce:'11'.repeat(32),issued_at:'100',expiry_time:'220' };
    const wire = (x = m) => base64.encode(encode('WalletChallengeV1',x));
    assert.equal(key.challenge(wire(),ctx,m.server_origin,'219').wire_base64,wire());
    assert.throws(() => key.challenge(wire(),ctx,m.server_origin,'220'),/CHALLENGE_EXPIRED/);
    for (const changed of [{genesis_hash:'22'.repeat(32)},{audience:'foreign'},{server_origin:'https://foreign.test'},{owner:base64.encode(new Uint8Array(20))}]) assert.throws(() => key.challenge(wire({...m,...changed}),ctx,m.server_origin,'150'),/CHALLENGE_CONTEXT/);
  } finally { key.destroy(); }
});
test('account generation drops old replies before they touch private state', () => {
  const s = setup(), old = s.generation; s.select('other'); assert.equal(s.accept(view(),old,100),false); assert.equal(s.view,undefined);
  assert.throws(() => s.accept(view(),s.generation,100),/ACCOUNT_MISMATCH/);
});
test('reverse sequence does not roll back balances and identical seq conflicting values are rejected', () => {
  const s = setup(); assert(s.accept(view(),s.generation,100)); assert.equal(s.accept(view('owner','1'),s.generation,101),false);
  const conflict = view(); conflict.ledger[0].P='71'; assert.throws(() => s.accept(conflict,s.generation,102),/SNAPSHOT_CONFLICT/); assert.equal(s.view!.ledger[0].P,'70');
});
test('health revision changes at unchanged economic seq, stale and reconnect close/open admission', () => {
  const s = setup(); s.accept(view(),s.generation,100); assert(s.open(5080)); assert(!s.open(5081));
  const stale = view(); stale.status.revision='11'; stale.status.mode='STALE'; stale.status.reason='RPC_UNAVAILABLE'; stale.status.observation.fresh=false;
  assert(s.accept(stale,s.generation,200)); assert(!s.open(200)); assert.equal(s.accept(view(),s.generation,201),false);
  const fresh = view(); fresh.status.revision='12'; assert(s.accept(fresh,s.generation,300)); assert(s.open(300)); s.disconnect('DISCONNECTED',s.generation); assert(!s.open(301)); assert(s.view);
});
test('asset conservation excludes P and rejects crossed contexts or snapshot heights', () => {
  for (const mutate of [(v:View)=>{v.ledger[0].A='120'},(v:View)=>{v.ledger[0].R='101'},(v:View)=>{v.context={...ctx,genesis_hash:'ef'.repeat(32)}},(v:View)=>{v.status={...v.status,observation:{...v.status.observation,observed_height:'13'}}}]) {
    const s=setup(), v=view(); mutate(v); assert.throws(()=>s.accept(v,s.generation,100)); assert.equal(s.view,undefined);
  }
});
test('public book verifies framed canonical hash and monotonic sequence', () => {
  const s=setup(); assert(s.acceptBook(book(),s.generation)); assert.equal(s.acceptBook(book('1'),s.generation),false);
  const bad=book(); bad.asks=[{price_ticks:'1',qty_lots:'1',order_count:'1'}]; assert.throws(()=>s.acceptBook(bad,s.generation),/BOOK_HASH/);
});
test('all approved S2 signed vectors verify with the existing browser codec', () => {
  const vectors=JSON.parse(readFileSync(new URL('../../protocol/s2/vectors/signed.json',import.meta.url),'utf8'));
  for (const item of vectors.cases) {
    const raw=hexToBytes(item.canonical_hex); const name = item.id === 'cancel' ? 'CancelV1' : item.id === 'wallet' ? 'WalletChallengeV1' : 'OrderV1';
    assert.deepEqual(encode(name,decode(name,raw)),raw);
    assert(ml_dsa65.verify(hexToBytes(item.public_key_hex),hexToBytes(item.sign_input_hex),hexToBytes(item.signature_hex)));
  }
});

test('lost response keeps same signed bytes, blocks new ID, rejects foreign receipt and account-switch completion', async () => {
  const { TradingClient } = await import('./client.ts');
  const key = new TradingKey(), other = new TradingKey();
  const sent: string[] = []; let lose = true, foreign = true;
  let receipt: Record<string,unknown>;
  const response = (value: unknown,status=200) => new Response(JSON.stringify(value),{status});
  const transport: typeof fetch = async (input,init) => {
    const path=String(input), payload=init?.body ? JSON.parse(String(init.body)) : undefined;
    if (path==='/s2/network') return response({context:ctx,profile:'s2-local-v1'});
    if (path==='/s2/auth/challenges') {
      const now=Math.floor(Date.now()/1000);
      return response({wire_base64:base64.encode(encode('WalletChallengeV1',{protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,server_origin:payload.origin,audience:'exchange-api',owner:payload.owner,challenge_nonce:'45'.repeat(32),issued_at:String(now),expiry_time:String(now+120)}))});
    }
    if (path==='/s2/auth/sessions') return response({token:base64.encode(new Uint8Array(32)),owner:key.owner,origin:'http://127.0.0.1:5173',audience:'exchange-api',genesis_hash:ctx.genesis_hash,expiry_time:String(Math.floor(Date.now()/1000)+300)});
    if (path==='/s2/orders') {
      sent.push(String(init?.body)); const m=decode('OrderV1',base64.decode(payload.wire_base64));
      receipt={context:ctx,kind:'ORDER',request_id:m.order_id,request_hash:bytesToHex(sha256(frame('NUS/ORDER/V1',base64.decode(payload.wire_base64)))),owner:key.owner,owner_epoch:'0',command_seq:'3',state:'LOCAL_ACCEPTED',code:'OK',durability:'LOCAL_FSYNC',replicated:false,observed_height:'12',snapshot_id:'cd'.repeat(32),result_hash:'de'.repeat(32),journal_commit_hash:'ef'.repeat(32)};
      if (lose) {lose=false; throw Error('response lost');} return response(receipt);
    }
    if (path.startsWith('/s2/me/commands/')) return response(foreign ? {...receipt,owner:other.owner} : receipt);
    return response({});
  };
  const c=new TradingClient(ctx.genesis_hash,'http://127.0.0.1:5173',transport,[key,other]);
  try {
    await c.login(); c.views.accept(view(key.owner),c.views.generation,Date.now());
    const entry=await c.order('SELL','GTC','2','10'); assert.equal(entry.state,'SUBMISSION_UNKNOWN');
    await assert.rejects(()=>c.order('SELL','GTC','1','10'),/UNKNOWN_RECEIPT_REQUIRED/);
    await c.resolve(entry); assert.equal(entry.state,'SUBMISSION_UNKNOWN');
    await c.retry(entry); assert.equal(entry.state,'LOCAL_ACCEPTED'); assert.equal(sent.length,2); assert.equal(sent[0],sent[1]);
    c.select(1); assert.equal(c.authenticated,false); assert.equal(c.views.view,undefined);
    await assert.rejects(()=>c.resolve(entry),/ACCOUNT_MISMATCH/);
    // Serialized network payloads contain only challenge/order public bytes, never raw secret material.
    for (const body of sent) assert.deepEqual(Object.keys(JSON.parse(body)).sort(),['context','signature_base64','wire_base64']);
  } finally { c.close(); }
});

test('late login reply after account switch cannot install another account session', async()=>{
  const { TradingClient }=await import('./client.ts');
  let release!: (r:Response)=>void;
  const response=new Promise<Response>(resolve=>{release=resolve;});
  let calls=0;
  const c=new TradingClient(ctx.genesis_hash,'http://127.0.0.1:5173',async()=>{calls++;return response;});
  try {
    const pending=c.login(); c.select(1);
    release(new Response(JSON.stringify({context:ctx,profile:'s2-local-v1'})));
    await pending; assert.equal(c.authenticated,false); assert.equal(c.selected,1); assert.equal(calls,1); assert.equal(c.views.view,undefined);
  } finally {c.close();}
});

test('CTO-S2E-01: cumulative age, delivery budget, exact boundary and latched clock rollback', () => {
  for (const field of ['last_success_age_ms','block_age_ms'] as const) {
    const s=setup(), v=view(); v.status.observation[field]='4900';
    s.accept(v,s.generation,10000);
    assert(s.open(10100)); assert(!s.open(10101)); assert(!s.open(11000));
    s.accept(v,s.generation,12000,11900); assert(s.open(12000)); assert(!s.open(12001));
    s.accept(v,s.generation,14000,13900,101); assert(!s.open(14000));
  }
  const s=setup(); s.accept(view(),s.generation,10000); assert(s.open(10100));
  assert(!s.open(10099)); assert(!s.open(10200)); assert.equal(s.reason,'CLOCK_REGRESSION');
  assert(!s.accept(view(),s.generation,9000,10000));
  assert(s.accept(view(),s.generation,11000)); assert(s.open(11000));
  s.disconnect('DISCONNECTED',s.generation); assert(!s.open(11001));
  assert(s.accept(view(),s.generation,12000)); assert(s.open(12000));
  let monotonic=0; const m=new Views(ctx,()=>monotonic); m.select('owner'); m.accept(view(),m.generation,10000);
  monotonic=5001; assert(!m.open(10000)); // stationary wall clock cannot prolong admission
});

test('expired observation during delayed polling signs and posts zero new Orders; fresh reconnect reopens', async t => {
  const { TradingClient }=await import('./client.ts');
  let now=100000, mono=0, age='4900', delay=0, signatures=0, posts=0;
  t.mock.method(Date,'now',()=>now); t.mock.method(performance,'now',()=>mono);
  const key=new TradingKey();
  t.mock.method(key,'order',()=>{signatures++; throw Error('unexpected signature');});
  const response=(v:unknown)=>new Response(JSON.stringify(v));
  const c=new TradingClient(ctx.genesis_hash,'http://127.0.0.1:5173',async(input,init)=>{
    const path=String(input), payload=init?.body?JSON.parse(String(init.body)):undefined;
    if(path==='/s2/network')return response({context:ctx,profile:'s2-local-v1'});
    if(path==='/s2/auth/challenges')return response({wire_base64:base64.encode(encode('WalletChallengeV1',{protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,server_origin:payload.origin,audience:'exchange-api',owner:key.owner,challenge_nonce:'45'.repeat(32),issued_at:'100',expiry_time:'220'}))});
    if(path==='/s2/auth/sessions')return response({token:base64.encode(new Uint8Array(32)),owner:key.owner,origin:'http://127.0.0.1:5173',audience:'exchange-api',genesis_hash:ctx.genesis_hash,expiry_time:'400'});
    if(path==='/s2/book')return response(book());
    if(path==='/s2/me') { const v=view(key.owner);v.status.revision=String(now);v.status.observation.last_success_age_ms=age;now+=delay;mono+=delay;return response(v); }
    if(path==='/s2/orders')posts++;
    return response({});
  },[key]);
  try {
    await c.login(); await c.refresh(); assert(c.views.open(now));
    now+=1000;mono+=1000;
    await assert.rejects(()=>c.order('BUY','GTC','1','10'),/ADMISSION_CLOSED/);
    delay=101;await c.refresh();assert(!c.views.open(now));
    await assert.rejects(()=>c.order('BUY','GTC','1','10'),/ADMISSION_CLOSED/);
    assert.equal(signatures,0);assert.equal(posts,0);
    age='0';delay=10;await c.refresh();assert(c.views.open(now));
    now--;assert(!c.views.open(now));now+=2;assert(!c.views.open(now));
    await c.refresh();assert(c.views.open(now));
  } finally {c.close();}
});
