import { chromium } from 'playwright-core';
import { ml_dsa65 } from '@noble/post-quantum/ml-dsa';
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
// Bounded integration test: only public evidence is exported; all child services stop.
const root=resolve(dirname(fileURLToPath(import.meta.url)), '../..'), web=resolve(root,'web');
const out=resolve(process.argv[2] ?? resolve(root,'.evidence/wallet'));
const scratch=process.env.PAPERCLIP_RUN_SCRATCH_DIR ?? process.env.S1_TEST_SCRATCH;
if (!scratch) throw Error('Set S1_TEST_SCRATCH to a fresh private local directory');
const home=resolve(scratch,'wallet-home');
mkdirSync(out,{recursive:true});
const env=Object.fromEntries(Object.entries(process.env).filter(([k])=>['PATH','HOME','TMPDIR','LANG'].includes(k)));
const children=[]; let browser, page; const report={status:'FAIL',transactions:[],checks:[]};
const hash=b=>createHash('sha256').update(b).digest('hex');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(f){let error;for(let i=0;i<100;i++){try{const v=await f();if(v)return v;}catch(e){error=e;}await sleep(300);}throw error??Error('deadline');}
function child(cmd,args,cwd=root,extra={}){const p=spawn(cmd,args,{cwd,env:{...env,...extra},stdio:['ignore','ignore','pipe']});let stderr='';p.stderr.on('data',b=>{stderr=(stderr+b).slice(-5000);});p.on('error',e=>{stderr=e.message;});children.push(p);p.diagnostic=()=>stderr;return p;}
async function get(path){const r=await fetch('http://127.0.0.1:18787'+path);assert.equal(r.status,200);return r.json();}
function fields(raw){let p=0;const out={};function v(){let n=0n,s=0n,b;do{b=raw[p++];n|=BigInt(b&127)<<s;s+=7n;}while(b&128);return n;}while(p<raw.length){const t=Number(v()),wire=t&7,tag=t>>3;if(wire===2){const n=Number(v());out[tag]=raw.subarray(p,p+n);p+=n;}else if(wire===0)out[tag]=v();else throw Error('wire');}return out;}
function vi(n){n=BigInt(n);const a=[];do{let b=Number(n&127n);n>>=7n;if(n)b|=128;a.push(b);}while(n);return Buffer.from(a);}
const bytes=(tag,b)=>Buffer.concat([vi(tag*8+2),vi(b.length),b]);
try {
 execFileSync(process.execPath,['s1/build.mjs'],{cwd:web,env});
 child(process.execPath,['s1/serve.mjs'],web,{S1_PORT:'18081',S1_API:'http://127.0.0.1:18787'});
 await until(async()=> (await fetch('http://127.0.0.1:18081')).ok);
 browser=await chromium.launch({executablePath:process.env.CHROME_BIN??'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,env});
 page=await browser.newPage({viewport:{width:1100,height:1000}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto('http://127.0.0.1:18081');await page.click('#create');
 const keys=JSON.parse(await page.locator('#public').textContent());assert.equal(keys.length,2);assert.notEqual(keys[0],keys[1]);
 writeFileSync(resolve(out,'public-keys.json'),JSON.stringify(keys));
 const binary=resolve(process.env.S1_BINARY ?? resolve(root,'chain/app/bin/nusd')),devnet=resolve(root,'ops/s1/devnet.py');
 execFileSync('python3',[devnet,'init','--home',home,'--binary',binary,'--operators',resolve(root,'chain/app/config/operator-accounts.json'),'--base-port','30656','--user-public-keys',resolve(out,'public-keys.json')],{env,timeout:60000});
 const manifest=JSON.parse(readFileSync(resolve(home,'manifest.json')));report.manifest=manifest;
 child('python3',[devnet,'serve','--home',home]);
 await until(async()=>{const states=await Promise.all(manifest.nodes.map(async n=>(await (await fetch(n.rpc+'/status')).json()).result));return states.every(s=>Number(s.sync_info.latest_block_height)>=3);});
 const rpc=async(n,path)=>(await (await fetch(n.rpc+path)).json()).result;
 const h=(await rpc(manifest.nodes[0],'/status')).sync_info.latest_block_height;
 const blocks=await Promise.all(manifest.nodes.map(n=>rpc(n,'/block?height='+h)));assert.equal(new Set(blocks.map(b=>b.block_id.hash)).size,1);
 const validators=(await rpc(manifest.nodes[0],'/validators?height='+h)).validators;assert.equal(validators.length,4);
 report.consensus={height:h,block_hash:blocks[0].block_id.hash,validators};
 child('python3',[resolve(root,'settlement/s1/server.py'),'--rpc',manifest.nodes[0].rpc,'--genesis-hash',manifest.genesis_sha256,'--journal',resolve(home,'journal.sqlite'),'--port','18787','--origin','http://127.0.0.1:18081']);
 await until(()=>get('/s1/network')); await page.fill('#genesis',manifest.genesis_sha256);await page.click('#bind');
 await page.click('#refresh');await until(async()=> (await page.locator('#balance').textContent()) || (await page.locator('#status').textContent()).includes('MISMATCH'));
 report.initial_status=await page.locator('#status').textContent();
 if(!(await page.locator('#balance').textContent())){await page.screenshot({path:resolve(out,'failure.png'),fullPage:true});throw Error(report.initial_status);}
 let hidden=false,lose=false;const posts=[];
 await page.route('**/s1/txs',async route=>{posts.push(JSON.parse(route.request().postData()).tx_bytes);const r=await route.fetch();if(lose){lose=false;await route.abort('failed');}else await route.fulfill({response:r});});
 await page.route(/\/s1\/txs\/[A-F0-9]{64}$/,async route=>{if(hidden)await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({tx_hash:route.request().url().split('/').pop(),state:'SUBMISSION_UNKNOWN',height:null,code:null})});else await route.continue();});
 report.balances=[]; report.excessWithdrawals=[];
 for(let user=0;user<2;user++){
  await page.selectOption('#account',String(user));await page.click('#refresh');await until(async()=> (await page.locator('#balance').textContent()).includes('sequence 0'));
  const owner=(await page.locator('#balance').textContent()).match(/주소 (nus1\w+)/)[1];
  for(const [operation,amount,expected] of [['DEPOSIT','100','100000000'],['WITHDRAW','40','60000000']]){
   const before=await get('/s1/accounts/'+owner),count=posts.length;hidden=true;lose=true;
   await page.selectOption('#operation',operation);await page.fill('#amount',amount);
   await page.evaluate(()=>{document.querySelector('#submit').click();document.querySelector('#submit').click();});
   await until(async()=>posts.length===count+1 && (await page.locator('#status').textContent()).startsWith('제출 후'));
   const raw=Buffer.from(posts.at(-1),'base64'),digest=hash(raw).toUpperCase();
   await page.click('#resolve');await until(async()=> (await page.locator('#status').textContent()).startsWith('TX 조회 완료'));
   assert.ok((await page.locator('#history').textContent()).includes('확인 불가'));
   await page.click('#submit');await until(async()=> (await page.locator('#status').textContent()).includes('미확인 TX'));assert.equal(posts.length,count+1);
   await page.screenshot({path:resolve(out,`${user}-${operation}-unknown.png`),fullPage:true});
   hidden=false;
   const tx=await until(async()=>{const t=await get('/s1/txs/'+digest);return t.state==='COMMITTED'?t:false;});
   await page.click('#resolve');await until(async()=> !(await page.locator('#history').textContent()).includes('확인 불가'));
   const after=await get('/s1/accounts/'+owner);assert.equal(after.exchange_atoms,expected);assert.equal(BigInt(after.sequence),BigInt(before.sequence)+1n);assert.equal(BigInt(after.epoch),BigInt(before.epoch)+(operation==='WITHDRAW'?1n:0n));
   const f=fields(raw),signDoc=Buffer.concat([bytes(1,f[1]),bytes(2,f[2]),bytes(3,Buffer.from('nus-s1-dev-1')),...(before.account_number==='0'?[]:[Buffer.concat([vi(32),vi(before.account_number)])])]);
   assert.ok(ml_dsa65.verify(Buffer.from(keys[user],'base64'),signDoc,f[3]));
   const msg=fields(fields(fields(f[1])[1])[2]),rid=msg[4].toString('hex');assert.equal(msg[3].toString(),String(BigInt(amount)*1000000n));assert.equal(msg[5].toString(),before.epoch);
   const signer=fields(fields(f[2])[1]);assert.equal(String(signer[3]??0n),before.sequence);
   const receipt=await get(`/s1/accounts/${owner}/requests/${rid}`);assert.equal(receipt.committed_height,tx.height);
   const onchain=(await rpc(manifest.nodes[0],'/tx?hash=0x'+digest));assert.equal(onchain.tx,posts.at(-1));
   const replay=await fetch('http://127.0.0.1:18787/s1/txs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({tx_bytes:posts.at(-1)})});assert.equal((await replay.json()).tx_hash,digest);
   const again=await get('/s1/accounts/'+owner);for(const k of ['sequence','epoch','bank_atoms','exchange_atoms','gas_atoms'])assert.equal(again[k],after[k]);
   report.transactions.push({user,operation,amount,before,after,tx,receipt,tx_bytes:posts.at(-1),sign_doc_hex:signDoc.toString('hex'),signature_verified:true,duplicate_click_posts:posts.length-count});
  }
  await page.click('#refresh');await until(async()=> (await page.locator('#balance').textContent()).includes('거래소 확정 60.000000'));
  const count=posts.length, beforeExcess=await get('/s1/accounts/'+owner);
  await page.selectOption('#operation','WITHDRAW');await page.fill('#amount','60.000001');
  await page.click('#submit');await until(async()=> (await page.locator('#status').textContent())==='INSUFFICIENT_BALANCE');
  assert.equal(posts.length,count);
  const afterExcess=await get('/s1/accounts/'+owner);
  for(const k of ['sequence','epoch','bank_atoms','exchange_atoms','gas_atoms'])assert.equal(afterExcess[k],beforeExcess[k]);
  report.excessWithdrawals.push({user,amount:'60.000001',state:'LOCAL_REJECTED',newPosts:posts.length-count,before:beforeExcess,after:afterExcess});
  await page.screenshot({path:resolve(out,`user-${user}-final.png`),fullPage:true});report.balances.push(await get('/s1/accounts/'+owner));
 }
 await page.setViewportSize({width:390,height:844});
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
 await page.screenshot({path:resolve(out,'mobile.png'),fullPage:true});
 await page.click('#reset');assert.equal(await page.locator('#public').textContent(),'');assert.equal(await page.locator('#history').textContent(),'');
 assert.deepEqual(errors,[]);report.status='PASS';report.browser=browser.version();report.checks=['browser CSPRNG public keys only','fresh four-validator consensus','each user 100 deposit / 40 withdraw / 60 committed','response loss after real POST','UNKNOWN lock prevents new sequence/signature','double click submits once','original TX hash resolution','on-chain bytes and ML-DSA signature reconciliation','same bytes replay leaves balances and sequence unchanged','both excess withdrawals rejected before signing with unchanged ledger','390px layout','reset after resolved TX clears session'];
}catch(e){report.error=String(e);report.screen=await page?.locator('body').innerText();await page?.screenshot({path:resolve(out,'failure.png'),fullPage:true});process.exitCode=1;}
finally{
 await browser?.close();
 for(const p of children.reverse()){if(p.exitCode!==null||p.signalCode!==null)continue;p.kill('SIGTERM');await Promise.race([new Promise(r=>p.once('exit',r)),sleep(20000)]);if(p.exitCode===null&&p.signalCode===null){p.kill('SIGKILL');await new Promise(r=>p.once('exit',r));}}
 report.cleanup=children.map(p=>({pid:p.pid,exitCode:p.exitCode,signalCode:p.signalCode}));
 report.source_hashes=Object.fromEntries(['settlement/s1/server.py','ops/s1/devnet.py','web/s1/client.ts','web/s1/direct.ts','web/s1/browser.ts','web/dist/s1/wallet.js','web/package-lock.json','chain/app/go.mod','chain/app/go.sum'].map(p=>[p,hash(readFileSync(resolve(root,p)))]));
 writeFileSync(resolve(out,'result.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({screen:report.screen,status:report.status,error:report.error,transactions:report.transactions.length,cleanup:report.cleanup}));
}
