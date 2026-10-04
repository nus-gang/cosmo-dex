// Independent HTTP concurrency oracle. Separate from literal UI guide replay.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {writeFile} from 'node:fs/promises';
import path from 'node:path';
const require=createRequire(path.resolve('web/package.json'));
export default async function({page,control,output}) {
 const report={result:'FAIL',scope:'real native Node HTTP -> Python adapter -> Rust engine; synthetic assets; CORS enforcement is tested in separate literal browser run',started:new Date().toISOString(),steps:[]};
 const equal=(a,b,label)=>assert.deepEqual(a,b,label);
 function invariant(v){for(const r of v.ledger){equal((BigInt(r.C)-BigInt(r.R)-BigInt(r.D)).toString(),r.A);assert(BigInt(r.A)>=0n);}}
 const economic=v=>({ledger:v.ledger,orders:v.orders,fills:v.fills,owner_epoch:v.owner_epoch});
 const row=(v,d)=>v.ledger.find(x=>x.denom===d);
 let qa; async function call(name,...args){return qa[name](...args);}
 try {
  const {build}=require('esbuild');
  const bundled=await build({stdin:{contents:`
   import {TradingKey,context,base64,requestId} from './web/s2/session.ts';
   import {TradingClient} from './web/s2/client.ts';
   import {sha256} from './web/node_modules/@noble/hashes/sha256.js';
   const keys=[new TradingKey(),new TradingKey()],tokens=['',''];let clients=[],saved=[];
   const api='http://127.0.0.1:8788';
   const hex=x=>Array.from(x,b=>b.toString(16).padStart(2,'0')).join('');
   export const publicKeys=()=>keys.map(k=>base64.encode(k.publicKey));
   export async function bind(genesis){clients=keys.map((_,i)=>{const c=new TradingClient(genesis,'http://127.0.0.1:5173',(p,init)=>{if(init?.headers?.Authorization)tokens[i]=init.headers.Authorization;return fetch(new URL(String(p),api),{...init,headers:{...init?.headers,Origin:'http://127.0.0.1:5173'}});},keys);c.select(i);return c;});for(const c of clients){await c.login();await c.refresh();}}
   export async function login(){for(const c of clients){await c.login();await c.refresh();}}
   export async function view(i){await clients[i].refresh();return clients[i].views.view;}
   export async function tx(i,operation,denom,amount,direct=true){const c=clients[i];const e=await c.transfer(operation,denom,amount,direct);const end=Date.now()+30000;while(e.state==='SUBMISSION_UNKNOWN'&&Date.now()<end){await c.direct.resolve(e);await new Promise(r=>setTimeout(r,150));}if(e.state!=='COMMITTED')throw Error('TX not committed '+e.state);return {state:e.state,tx_hash:e.tx_hash,height:e.height,input:e.input,account:await c.direct.account(c.key)};}
   async function send(i,body,index){const start=performance.now();const r=await fetch(api+'/s2/orders',{method:'POST',headers:{'Content-Type':'application/json',Authorization:tokens[i],Origin:'http://127.0.0.1:5173'},body:JSON.stringify(body)});return {index,start_ms:start,end_ms:performance.now(),http:r.status,body:await r.json(),signed_body_sha256:hex(sha256(new TextEncoder().encode(JSON.stringify(body))))};}
   export async function burst(){const c=clients[0];await c.refresh();const v=c.views.view;saved=Array.from({length:12},(_,i)=>({id:(i+1).toString(16).padStart(64,'0'),body:keys[0].order(c.views.ctx,v.owner_epoch,v.observed_height,'SELL','GTC','1','10',(i+1).toString(16).padStart(64,'0'))}));return Promise.all(saved.map((s,i)=>send(0,s.body,i)));}
   export async function retry(){return Promise.all(saved.map((s,i)=>send(0,s.body,i)));}
   export async function negatives(){const c=clients[0];await c.refresh();const v=c.views.view;const conflict=keys[0].order(c.views.ctx,v.owner_epoch,v.observed_height,'SELL','GTC','2','10',saved.find(s=>s.id===v.orders[0].order_id)?.id??v.orders[0].order_id);const invalid=keys[0].order(c.views.ctx,v.owner_epoch,v.observed_height,'SELL','GTC','1','10');const bytes=base64.decode(invalid.signature_base64);bytes[0]^=1;invalid.signature_base64=base64.encode(bytes);return [await send(0,conflict,0),await send(0,invalid,1)];}
   export async function order(i,side,tif,qty,price){const c=clients[i];await c.refresh();const e=await c.order(side,tif,qty,price);return {id:e.id,state:e.state,receipt:e.receipt};}
   export async function cancelAll(i){const c=clients[i];await c.refresh();const ids=c.views.view.orders.filter(o=>['OPEN','PARTIALLY_FILLED'].includes(o.state)).map(o=>o.order_id);const results=[];for(const id of ids){await c.refresh();const e=await c.cancel(id);results.push({id:e.id,state:e.state,receipt:e.receipt});}return results;}
  `,resolveDir:process.cwd(),sourcefile:'qa-http-browser.ts'},bundle:true,write:false,platform:'node',format:'esm',target:'es2022'});
  await control('temporary_start');await page.goto('http://127.0.0.1:5173');qa=await import('data:text/javascript;base64,'+Buffer.from(bundled.outputFiles[0].text).toString('base64'));
  const publicKeys=await call('publicKeys');await writeFile(path.join(output,'public-keys.json'),JSON.stringify(publicKeys,null,2));
  report.pins=await control('init',{publicKeys});await control('temporary_stop');report.firstHealth=await control('start');await call('bind',report.pins.chain_genesis);
  report.initial=[await call('view',0),await call('view',1)];
  report.deposits=[await call('tx',0,'DEPOSIT','DEVBASE','10000000'),await call('tx',1,'DEPOSIT','DEVQUOTE','100000000')];
  await page.waitForTimeout(1200);report.before=[await call('view',0),await call('view',1)];report.before.forEach(invariant);
  equal(row(report.before[0],'DEVBASE').C,'10000000');equal(row(report.before[1],'DEVQUOTE').C,'100000000');
  report.burst=await call('burst');
  assert.equal(report.burst.length,12);assert(new Set(report.burst.map(x=>x.body.request_id)).size===12);
  // At least two requests must be issued before any response returns.
  assert(Math.max(...report.burst.map(x=>x.start_ms))<Math.min(...report.burst.map(x=>x.end_ms)),'all twelve HTTP requests overlap');
  const accepted=report.burst.filter(x=>x.body.state==='LOCAL_ACCEPTED');const rejected=report.burst.filter(x=>x.body.state==='REJECTED');
  equal(accepted.length,10,'10 available BASE reserve exactly 10 one-BASE requests');equal(rejected.length,2);
  assert(rejected.every(x=>x.body.code==='INSUFFICIENT_AVAILABLE'),'excess reservations rejected by ledger');
  report.afterBurst=[await call('view',0),await call('view',1)];report.afterBurst.forEach(invariant);
  equal(row(report.afterBurst[0],'DEVBASE'),{denom:'DEVBASE',C:'10000000',R:'10000000',D:'0',P:'0',A:'0'});
  equal(report.afterBurst[0].orders.length,10);report.retry=await call('retry');
  report.afterRetry=[await call('view',0),await call('view',1)];report.afterRetry.forEach(invariant);
  equal(economic(report.afterRetry[0]),economic(report.afterBurst[0]),'retry has one effect');
  for(let i=0;i<12;i++)for(const k of ['request_id','request_hash','state','code','command_seq','result_hash','journal_commit_hash'])equal(report.retry[i].body[k],report.burst[i].body[k],'identical retry '+k);
  report.negatives=await call('negatives');assert(report.negatives.every(x=>x.http>=400));equal(economic(await call('view',0)),economic(report.afterRetry[0]),'bad signature and same ID different body cannot change ledger');
  // Same-price admission FIFO and actual partial matching.
  report.buy=await call('order',1,'BUY','GTC','1.5','10');equal(report.buy.state,'LOCAL_ACCEPTED');
  report.matched=[await call('view',0),await call('view',1)];report.matched.forEach(invariant);
  const sorted=[...report.afterBurst[0].orders].sort((a,b)=>Number(BigInt(a.admission_seq)-BigInt(b.admission_seq)));
  equal(report.matched[0].orders.find(o=>o.order_id===sorted[0].order_id).filled_qty_lots,'1000');equal(report.matched[0].orders.find(o=>o.order_id===sorted[1].order_id).filled_qty_lots,'500');
  equal(row(report.matched[0],'DEVBASE').D,'1500000');equal(row(report.matched[1],'DEVBASE').P,'1500000');
  report.pendingSell=await call('order',1,'SELL','GTC','0.001','10');equal(report.pendingSell.state,'REJECTED');
  // Direct confirmed withdrawal is not subject to an engine permission.
  report.directWithdraw=await call('tx',1,'WITHDRAW','DEVQUOTE','1000000');
  const deadline=Date.now()+30000;
  do {await page.waitForTimeout(300);report.corrected=[await call('view',0),await call('view',1)];}while(report.corrected.some(v=>v.fills.some(f=>f.state!=='CORRECTED'))&&Date.now()<deadline);
  for(const v of report.corrected){invariant(v);assert(v.fills.length===2);assert(v.fills.every(f=>f.state==='CORRECTED'));for(const r of v.ledger){equal(r.D,'0');equal(r.P,'0');}}
  equal(row(report.corrected[1],'DEVQUOTE').C,'99000000');assert(BigInt(report.corrected[1].owner_epoch)>BigInt(report.matched[1].owner_epoch));
  report.cancelRemaining=await call('cancelAll',0);
  report.normalWithdraw=await call('tx',0,'WITHDRAW','DEVBASE','1000000',false);
  await page.waitForTimeout(1200);report.beforeRestart=[await call('view',0),await call('view',1)];report.beforeRestart.forEach(invariant);
  report.firstStop=await control('stop');report.restartHealth=await control('start');await call('login');report.afterRestart=[await call('view',0),await call('view',1)];
  for(let i=0;i<2;i++)equal(economic(report.afterRestart[i]),economic(report.beforeRestart[i]),'restart ledger/order/fill preservation');
  equal(await call('publicKeys'),publicKeys);report.secondStop=await control('stop');report.result='PASS';
  return {scope:report.scope,concurrentHTTP:'PASS',documentQA:'NOT_RUN',accepted:10,rejected:2};
 }catch(e){report.error=String(e);throw e;}
 finally{report.finished=new Date().toISOString();await writeFile(path.join(output,'qa-http.json'),JSON.stringify(report,null,2)+'\n');}
}
