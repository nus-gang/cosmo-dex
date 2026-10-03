// Bounded, synthetic-assets-only real chain/browser acceptance. No Wallet edits.
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const web = resolve(process.env.S2_TEST_WEB ?? resolve(root,'web'));
const require = createRequire(resolve(web,'package.json'));
const { chromium } = require('playwright-core'), { build } = require('esbuild');
const scratch = process.env.PAPERCLIP_RUN_SCRATCH_DIR ?? process.env.S2_TEST_SCRATCH;
if (!scratch) throw Error('private scratch required');
const out = resolve(process.argv[2]), home = resolve(scratch,'account-browser-node-'+Date.now());
mkdirSync(out,{recursive:true});
const chain = resolve(process.env.S2_TEST_CHAIN ?? resolve(root,'chain/app/bin/nusd'));
const rpcPort = Number(process.env.S2_TEST_RPC_PORT ?? 30457), apiPort = rpcPort + 2;
const rpc = `http://127.0.0.1:${rpcPort}`, api = `http://127.0.0.1:${apiPort}`;
const children = [], report = {result:'FAIL', transactions:[]};
let browser, pageServer;
const hash = raw => createHash('sha256').update(raw).digest('hex');
const save = (name,value) => writeFileSync(resolve(out,name),JSON.stringify(value,null,2)+'\n');
const sleep = ms => new Promise(r=>setTimeout(r,ms));
async function until(f) { let error; for(let i=0;i<100;i++){try{const x=await f();if(x)return x;}catch(e){error=e;} await sleep(200);} throw error??Error('deadline'); }
function child(cmd,args){const p=spawn(cmd,args,{cwd:root,stdio:['ignore','ignore','pipe']});let diagnostic='';p.stderr.on('data',b=>{diagnostic=(diagnostic+b).slice(-3000);});p.on('error',e=>{diagnostic=String(e);});p.diagnostic=()=>diagnostic;children.push(p);return p;}
async function get(path){const r=await fetch(api+path); if(r.status!==200)throw Error(`HTTP ${r.status}: ${await r.text()}`);return r.json();}
try {
 const source=readFileSync(resolve(root,'settlement/s2/account_browser.js'),'utf8');
 const bundle=await build({stdin:{contents:source,resolveDir:web},bundle:true,write:false,format:'iife',platform:'browser'});
 browser=await chromium.launch({headless:true,...(process.env.CHROME_BIN?{executablePath:process.env.CHROME_BIN}:{})});
 const page=await browser.newPage();
 report.browserErrors=[];page.on('console',msg=>{if(msg.type()==='error')report.browserErrors.push(msg.text());});page.on('requestfailed',req=>report.browserErrors.push(req.url()+': '+req.failure().errorText));
 pageServer=createServer((req,res)=>{res.setHeader('Content-Type','text/html');res.end('<!doctype html><title>NUS-48 DIRECT test</title><p>Browser-only signing; public evidence only.</p>');});
 await new Promise((ok,fail)=>{pageServer.once('error',fail);pageServer.listen(5173,'127.0.0.1',ok);});
 await page.goto('http://127.0.0.1:5173/nus48');await page.addScriptTag({content:bundle.outputFiles[0].text});
 const keys=await page.evaluate(()=>window.accountTest.publicKeys), owners=await page.evaluate(()=>window.accountTest.owners);
 const keysFile=resolve(out,'public-keys.json');save('public-keys.json',keys);
 const initial=JSON.parse(execFileSync(chain,['init','--network','s2','--home',home,'--rpc',`tcp://127.0.0.1:${rpcPort}`,'--p2p',`tcp://127.0.0.1:${rpcPort-1}`,'--operator-accounts',resolve(root,'chain/app/config/operator-accounts.json'),'--user-public-keys',keysFile]));
 save('init.json',initial);
 const genesis=resolve(home,'config/genesis.json');writeFileSync(resolve(out,'genesis.json'),readFileSync(genesis));
 const node=child(chain,['start','--network','s2','--home',home,'--genesis-hash',initial.genesis_hash]);
 await until(async()=>{if(node.exitCode!==null)throw Error(node.diagnostic());const r=await fetch(rpc+'/status');const s=await r.json();return BigInt(s.result.sync_info.latest_block_height)>0n;});
 const server=child('python3',['settlement/s2/account_harness.py',genesis,rpc,resolve(out,'direct-evidence'),String(apiPort)]);
 await until(async()=>{if(server.exitCode!==null)throw Error(server.diagnostic());return get('/s2/accounts/'+owners[0]);});
 for(const user of [0,1]) for(const denom of ['DEVBASE','DEVQUOTE']) for(const operation of ['Deposit','Withdraw']) {
   const amount=operation==='Deposit'?'10000000':'1000000';
   const tx=await page.evaluate(args=>window.accountTest.transact(args),{api,user,denom,operation,amount});
   assert.equal(tx.before.context.genesis_hash,initial.genesis_hash);assert.equal(tx.before.public_key_base64,keys[user]);
   assert.equal(tx.submitted.tx_hash,tx.tx_hash);assert.equal(tx.submitted.state,'SUBMISSION_UNKNOWN');
   const committed=await until(async()=>{const value=await get('/s1/txs/'+tx.tx_hash);return value.state==='COMMITTED'?value:false;});
   const after=await until(async()=>{const v=await get('/s2/accounts/'+owners[user]);return BigInt(v.sequence)===BigInt(tx.before.sequence)+1n?v:false;});
   assert.equal(BigInt(after.owner_epoch),BigInt(tx.before.owner_epoch)+(operation==='Withdraw'?1n:0n));
   for(let i=0;i<2;i++){
     const b=tx.before.balances[i],a=after.balances[i],delta=b.denom===denom?BigInt(amount)*(operation==='Deposit'?1n:-1n):0n;
     assert.equal(BigInt(a.confirmed_atoms),BigInt(b.confirmed_atoms)+delta);assert.equal(BigInt(a.bank_atoms),BigInt(b.bank_atoms)-delta);
   }
   assert.equal(BigInt(after.gas_atoms),BigInt(tx.before.gas_atoms)-1000n);
   const receipt=await get(`/s1/accounts/${owners[user]}/requests/${tx.request_id}`);assert.equal(receipt.original_tx_hash,tx.tx_hash);
   const onchain=await (await fetch(rpc+'/tx?hash=0x'+tx.tx_hash)).json();assert.equal(onchain.result.tx,tx.tx_bytes);
   report.transactions.push({user,denom,operation,...tx,committed,after,receipt});
 }
 node.kill('SIGTERM');await new Promise(r=>node.once('exit',r));
 const outage=await fetch(api+'/s2/accounts/'+owners[0]);assert.equal(outage.status,503);report.outage=await outage.json();assert.equal(report.outage.signing_ready,false);
 report.result='PASS';report.browser=browser.version();report.genesis_hash=initial.genesis_hash;report.chain_binary_hash=hash(readFileSync(chain));report.scope='fresh single validator S2 genesis; browser CSPRNG keys; public REST -> browser DIRECT sign -> deposit/withdraw both assets and users; sequence/epoch/bank/C/GAS; matching engine absent; real RPC outage';
} catch(e){report.error=String(e);report.diagnostics=children.map(p=>p.diagnostic());process.exitCode=1;}
finally{
 await browser?.close();
 if(pageServer)await new Promise(r=>pageServer.close(r));
 for(const p of children.reverse()){if(p.exitCode!==null||p.signalCode!==null)continue;p.kill('SIGTERM');await Promise.race([new Promise(r=>p.once('exit',r)),sleep(5000)]);if(p.exitCode===null&&p.signalCode===null){p.kill('SIGKILL');await new Promise(r=>p.once('exit',r));}}
 report.cleanup=children.map(p=>({exit:p.exitCode,signal:p.signalCode}));save('result.json',report);console.log(JSON.stringify({result:report.result,transactions:report.transactions.length,error:report.error}));
}
