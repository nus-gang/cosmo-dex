// Bounded acceptance runner: child services always terminated, no persistent preview.
import { chromium } from 'playwright-core';
import { readFileSync,writeFileSync,mkdirSync } from 'node:fs';
import { resolve,dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn,execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const scratch=process.env.PAPERCLIP_RUN_SCRATCH_DIR??process.env.S2_TEST_SCRATCH;
if(!scratch)throw Error('scratch required');
const out=resolve(process.argv[2]);mkdirSync(out,{recursive:true});
const home=resolve(scratch,'wallet-node-'+Date.now());
const chain=resolve(process.env.S2_TEST_CHAIN??resolve(root,'../NUS-37/chain/app/bin/nusd'));
const engine=resolve(process.env.S2_ENGINE_BINARY??resolve(root,'../NUS-39/exchange/target/debug/exchange-s2'));
const rpc='http://127.0.0.1:30557',api='http://127.0.0.1:8788';
const four=process.env.S2_FOUR_VALIDATORS==='1';
const children=[],report={result:'FAIL',steps:[],observations:[],requests:[]};let browser,page,server;
const save=(name,v)=>writeFileSync(resolve(out,name),JSON.stringify(v,null,2)+'\n');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(f){let error;for(let i=0;i<100;i++){try{const x=await f();if(x)return x;}catch(e){error=e;}await sleep(200);}throw error??Error('deadline: '+await page.locator('#status').textContent());}
function child(cmd,args,cwd=root){const p=spawn(cmd,args,{cwd,env:Object.fromEntries(Object.entries(process.env).filter(([k])=>['PATH','HOME','TMPDIR','LANG'].includes(k))),stdio:['ignore','ignore','pipe']});let diagnostic='';p.stderr.on('data',b=>{diagnostic=(diagnostic+b).slice(-3000);});p.diagnostic=()=>diagnostic;children.push(p);return p;}
async function stop(p){if(p.exitCode!==null||p.signalCode!==null)return;p.kill('SIGTERM');await Promise.race([new Promise(r=>p.once('exit',r)),sleep(30000)]);if(p.exitCode===null&&p.signalCode===null){p.kill('SIGKILL');await new Promise(r=>p.once('exit',r));}}
async function text(id){return page.locator('#'+id).textContent();}
async function login(user){await page.selectOption('#account',String(user));await page.click('#login');await until(async()=>await text('status')==='계정 인증·조회 완료');await until(async()=>!(await page.locator('#order').isDisabled()));}
async function tx(operation,denom,amount){await page.selectOption('#asset',denom);await page.fill('#amount',amount);const n=await page.locator('#txs p').count();await page.click('#'+operation);await until(async()=>{
  if(await page.locator('#txs p').count()>n)return true;
  // A fail-closed account query may cross a block boundary. Retry the user
  // action only before any signed TX exists; UNKNOWN must never be re-signed.
  if(await text('status')==='DIRECT_UNAVAILABLE') {
    await Promise.all([
      page.waitForResponse(r=>r.url().startsWith(api+'/s2/accounts/')),
      page.click('#'+operation),
    ]);
  }
  return false;
});const row=page.locator('#txs p').last();await until(async()=>{const button=row.locator('button');if(await button.count())await button.click();return (await row.textContent()).includes('COMMITTED');});report.steps.push(await row.textContent());}
async function ledger(expected){await until(async()=>{const rows=await page.locator('#ledger tr').allTextContents();return rows.length===2;});const rows=await page.locator('#ledger tr').evaluateAll(rs=>rs.map(r=>Array.from(r.children,c=>c.textContent)));assert.deepEqual(rows,expected);report.steps.push('ledger '+JSON.stringify(rows));}
async function order(side,tif,qty,price){await page.selectOption('#side',side);await page.selectOption('#tif',tif);await page.fill('#qty',qty);await page.fill('#price',price);const n=await page.locator('#receipts p').count();await page.click('#order');await until(async()=>await page.locator('#receipts p').count()>n);await until(async()=>(await page.locator('#receipts p').last().textContent()).includes('LOCAL_ACCEPTED'));}
try{
 child(process.execPath,['web/s2/serve.mjs']);
 browser=await chromium.launch({headless:true,...(process.env.CHROME_BIN?{executablePath:process.env.CHROME_BIN}:{})});page=await browser.newPage();
 page.on('request',r=>{if(!r.url().startsWith(api))return;const body=r.postData();if(body){const v=JSON.parse(body);const allowed=r.url().endsWith('/s1/txs')?['tx_bytes']:r.url().endsWith('/s2/orders')||r.url().endsWith('/s2/cancels')?['context','wire_base64','signature_base64']:r.url().endsWith('/s2/auth/challenges')?['owner','origin','audience']:r.url().endsWith('/s2/auth/sessions')?['wire_base64','signature_base64']:['request_id'];assert.deepEqual(Object.keys(v).sort(),allowed.sort());}report.requests.push({path:new URL(r.url()).pathname,method:r.method(),bodyKeys:body?Object.keys(JSON.parse(body)):[]});});
 page.on('response',async r=>{const path=new URL(r.url()).pathname;if(r.url().startsWith(api)&&['/s2/me','/s2/book','/s2/orders','/s2/cancels','/s2/me/withdraw-prepare'].includes(path)){try{report.observations.push({path,status:r.status(),body:await r.json()});}catch{}}});
 await until(async()=>{try{await page.goto('http://127.0.0.1:5173');return true;}catch{return false;}});
 await page.click('#create');const keys=JSON.parse(await text('public'));save('public-keys.json',keys);
 let initial,genesis,node;
 if(four){
   const manifest=JSON.parse(execFileSync('python3',['ops/s1/devnet.py','init','--network','s2','--home',home,'--binary',chain,'--base-port','30556','--user-public-keys',resolve(out,'public-keys.json')],{cwd:root}));
   save('devnet-manifest.json',manifest);initial={genesis_hash:manifest.genesis_sha256};
   genesis=resolve(home,'node0/config/genesis.json');
   node=child('python3',['ops/s1/devnet.py','serve','--home',home]);
 }else{
   initial=JSON.parse(execFileSync(chain,['init','--network','s2','--home',home,'--rpc','tcp://127.0.0.1:30557','--p2p','tcp://127.0.0.1:30556','--operator-accounts',resolve(root,'chain/app/config/operator-accounts.json'),'--user-public-keys',resolve(out,'public-keys.json')]));
   genesis=resolve(home,'config/genesis.json');
   node=child(chain,['start','--network','s2','--home',home,'--genesis-hash',initial.genesis_hash]);
 }
 save('init.json',initial);writeFileSync(resolve(out,'genesis.json'),readFileSync(genesis));
 await until(async()=>{const r=await fetch(rpc+'/status');return BigInt((await r.json()).result.sync_info.latest_block_height)>0n;});
 if(four){
   let health;
   await until(async()=>{
     health=JSON.parse(execFileSync('python3',['ops/s1/devnet.py','health','--home',home],{cwd:root}));
     return health.length===4&&health.every(n=>!n.error&&BigInt(n.height)>0n&&!n.catching_up);
   });
   save('four-validator-health.json',health);
   const h=health.reduce((n,x)=>BigInt(x.height)<n?BigInt(x.height):n,BigInt(health[0].height));
   const blocks=await Promise.all([0,1,2,3].map(async i=>(await (await fetch(`http://127.0.0.1:${30557+i*10}/block?height=${h}`)).json()).result));
   assert.equal(new Set(blocks.map(b=>b.block_id.hash)).size,1);save('four-validator-blocks.json',blocks);
   report.steps.push('four validators same committed block');
 }
 execFileSync('python3',['settlement/s2/bootstrap.py','--genesis',genesis,'--output',resolve(out,'bootstrap'),'--rpc',rpc],{cwd:root});
 const args=['settlement/s2/server.py','--engine',engine,'--manifest',resolve(out,'bootstrap/manifest.json'),'--genesis',resolve(out,'bootstrap/genesis.json'),'--journal',resolve(out,'journal'),'--evidence',resolve(out,'rpc-evidence'),'--rpc',rpc,'--port','8788'];
 server=child('python3',[...args,'--bootstrap',resolve(out,'bootstrap/bootstrap.json')]);
 await until(async()=>{const r=await fetch(api+'/s2/status');return (await r.json()).mode==='OPEN';});
 await page.fill('#genesis',initial.genesis_hash);await page.click('#bind');
 await tx('deposit','DEVBASE','10');await tx('deposit','DEVQUOTE','100');
 await page.selectOption('#account','1');await tx('deposit','DEVBASE','10');await tx('deposit','DEVQUOTE','100');
 await login(0);await order('SELL','GTC','2','10');await until(async()=>(await text('orders')).includes('OPEN'));
 await login(1);await order('BUY','GTC','1','10');await until(async()=>(await text('fills')).includes('잠정'));await ledger([['DEVBASE','10000000','0','0','1000000','10000000'],['DEVQUOTE','100000000','0','10000000','0','90000000']]);await page.screenshot({path:resolve(out,'partial-buyer.png'),fullPage:true});
 await login(0);await until(async()=>(await text('orders')).includes('PARTIALLY_FILLED'));await ledger([['DEVBASE','10000000','1000000','1000000','0','8000000'],['DEVQUOTE','100000000','0','0','10000000','100000000']]);await page.locator('#orders button').first().click();await until(async()=>(await text('orders')).includes('CANCELLED_OFFCHAIN'));
 await order('SELL','GTC','0.5','10');await login(1);await order('BUY','IOC','1','10.001');await until(async()=>page.locator('#fills').textContent().then(s=>s.split('잠정').length>=3));
 await ledger([['DEVBASE','10000000','0','0','1500000','10000000'],['DEVQUOTE','100000000','0','15000500','0','84999500']]);
 await page.selectOption('#asset','DEVQUOTE');await page.fill('#amount','1');await page.click('#withdraw');await until(async()=>(await text('status')).includes('미정산 보류'));report.steps.push('normal withdraw: UNSETTLED_HOLD');
 await page.screenshot({path:resolve(out,'ioc-hold.png'),fullPage:true});
 await page.locator('summary').filter({hasText:'직접 출금'}).click();
 // Exercise a transient pre-signing snapshot rejection, then use the actual API.
 const postsBefore=report.requests.filter(r=>r.path==='/s1/txs'&&r.method==='POST').length;
 await page.route('**/s2/accounts/*',route=>route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({signing_ready:false})}),{times:1});
 await tx('direct-withdraw','DEVQUOTE','1');
 assert.equal(report.requests.filter(r=>r.path==='/s1/txs'&&r.method==='POST').length,postsBefore+1);
 report.steps.push('pre-signing unavailable retry emits exactly one signed TX');await until(async()=>(await text('fills')).includes('정정'));await page.screenshot({path:resolve(out,'corrected.png'),fullPage:true});
 await login(0);await until(async()=>(await text('fills')).includes('정정'));
 const before=await text('fills');await stop(server);await until(async()=>await page.locator('#order').isDisabled());report.steps.push('API outage closes order admission');
 server=child('python3',args);await until(async()=>{const r=await fetch(api+'/s2/status');return (await r.json()).mode==='OPEN';});
 await login(0);assert.equal(await text('fills'),before);report.steps.push('restart preserves corrected fill IDs');
 await tx('withdraw','DEVBASE','1');
 await until(async()=>{
   const response=page.waitForResponse(r=>r.url()===api+'/s2/me/withdraw-abort'&&r.request().method()==='POST');
   await page.click('#abort-withdraw');
   const receipt=await (await response).json();
   // The chain epoch observer can lag a committed DIRECT receipt. Retry only
   // a definitive STALE rejection, never a lost/unknown abort response.
   if(receipt.state==='REJECTED'&&receipt.code==='STALE')return false;
   assert.equal(receipt.state,'LOCAL_ACCEPTED');assert.equal(receipt.code,'OK');
   return true;
 });
 report.steps.push('withdraw abort confirmed by LOCAL_ACCEPTED receipt');
 // CTO-S2E-01: age the actual API view and delay delivery; polling cannot renew its budget.
 await login(0);
 const orderPostsBefore=report.requests.filter(r=>r.path==='/s2/orders'&&r.method==='POST').length;
 await page.route('**/s2/me',async route=>{
   const response=await route.fetch(), body=await response.json();
   body.status.observation.last_success_age_ms='4900';
   body.status.observation.block_age_ms='4900';
   await sleep(1100);await route.fulfill({response,json:body});
 });
 await until(async()=>await page.locator('#order').isDisabled());
 await sleep(1500);assert(await page.locator('#order').isDisabled());
 // Invoke the handler even though the native button is disabled: pre-signing guard must also close.
 await page.locator('#order').evaluate(b=>b.onclick(new MouseEvent('click')));
 assert.equal(await text('status'),'ADMISSION_CLOSED');
 assert.equal(report.requests.filter(r=>r.path==='/s2/orders'&&r.method==='POST').length,orderPostsBefore);
 await page.screenshot({path:resolve(out,'stale-admission.png'),fullPage:true});
 report.steps.push('CTO-S2E-01: 4900ms + delayed polling disables button and emits zero Order POST');
 await page.unrouteAll({behavior:'wait'});await login(0);
 report.steps.push('fresh API reconnect restores order admission');
 // Delayed old-account public response must never populate the switched account.
 let release;const gate=new Promise(r=>{release=r;});await page.route('**/s2/accounts/*',async route=>{const r=await route.fetch();await gate;await route.fulfill({response:r});});
 await page.click('#chain-account');await page.selectOption('#account','1');release();await sleep(400);assert.equal(await text('chain-balances'),'계정 전환 · 다시 조회하세요');await page.unroute('**/s2/accounts/*');
 assert.equal(await page.locator('#orders p').count(),0);assert.equal(await text('fills'),'');report.steps.push('account switch discards pending account response and private view');
 await stop(node);await page.click('#chain-account');await until(async()=>(await text('status')).includes('DIRECT_UNAVAILABLE'));report.steps.push('real RPC outage blocks DIRECT');
 assert.equal(await page.evaluate(()=>localStorage.length+sessionStorage.length),0);report.steps.push('no persistent browser key/session storage');
 report.result='PASS';report.genesis_hash=initial.genesis_hash;report.browser=browser.version();report.binary_hashes=Object.fromEntries([['chain',chain],['engine',engine]].map(([k,p])=>[k,createHash('sha256').update(readFileSync(p)).digest('hex')]));report.scope=(four?'four validators':'single validator')+' fresh S2 genesis; production browser UI + actual Rust/Python API; synthetic assets; not main QA';
}catch(e){report.error=String(e);report.stack=e.stack;report.status=page?await text('status').catch(()=>null):null;report.diagnostics=children.map(p=>p.diagnostic());process.exitCode=1;}
finally{await browser?.close();for(const p of children.reverse())await stop(p);save('result.json',report);console.log(JSON.stringify({result:report.result,steps:report.steps,error:report.error,status:report.status}));}
