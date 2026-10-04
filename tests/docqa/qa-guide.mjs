// Independent QA oracle from aad819f docs/s2-quickstart.md §§1–3/restart.
// Only real UI input; response observation excludes authentication and secret data.
import assert from 'node:assert/strict';
import {writeFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import path from 'node:path';
export default async function({page,control,output}) {
  const evidence={result:'FAIL',scope:'S2 documentation §§1–3 and same-home restart',started:new Date().toISOString(),steps:[],receipts:[],txs:[]};
  const responses=[]; let posts=0,navigations=0,phase='setup';
  page.on('request',r=>{if(new URL(r.url()).pathname==='/s1/txs'&&r.method()==='POST')posts++;});
  const allowed=new Set(['/s2/orders','/s2/cancels','/s2/me/withdraw-prepare','/s2/me/withdraw-abort']);
  page.on('response',r=>{const u=new URL(r.url()); if(allowed.has(u.pathname))responses.push(r.json().then(body=>{evidence.receipts.push({phase,path:u.pathname,http:r.status(),body});}).catch(()=>{}));});
  async function until(label,fn) { const end=Date.now()+30000; while(Date.now()<end){if(await fn())return;await page.waitForTimeout(150);}throw Error('timeout: '+label+'; UI status: '+await page.locator('#status').textContent()); }
  const text=id=>page.locator('#'+id).textContent();
  async function login(i) {await page.locator('#account').selectOption(String(i));await page.locator('#login').click();await until('login '+i,async()=>await text('status')==='계정 인증·조회 완료');}
  async function account() {await page.locator('#chain-account').click();await until('chain account',async()=>(await text('chain-balances')).includes('DEVGAS'));return text('chain-balances');}
  async function ledger() {return page.locator('#ledger tr').evaluateAll(rows=>rows.map(r=>Object.fromEntries(['denom','C','R','D','P','A'].map((k,i)=>[k,r.children[i].textContent]))));}
  function row(denom,C,R,D,P,A){return {denom,C:String(C),R:String(R),D:String(D),P:String(P),A:String(A)};}
  const expectA=(R,D,P,A)=>[row('DEVBASE',10000000,R,D,0,A),row('DEVQUOTE',0,0,0,P,0)];
  const expectB=(D,P,A)=>[row('DEVBASE',0,0,0,P,0),row('DEVQUOTE',100000000,0,D,0,A)];
  async function check(label,expected) {await until(label,async()=>JSON.stringify(await ledger())===JSON.stringify(expected));const actual=await ledger();for(const r of actual){assert.equal(BigInt(r.C)-BigInt(r.R)-BigInt(r.D),BigInt(r.A));assert(BigInt(r.A)>=0n);}evidence.steps.push({label,ledger:actual,orders:await text('orders'),fills:await text('fills'),book:await text('book')});}
  async function order(side,tif,qty,price,expected='LOCAL_ACCEPTED') {const previous=await page.locator('#receipts p').count();await page.locator('#side').selectOption(side);await page.locator('#tif').selectOption(tif);await page.locator('#qty').fill(qty);await page.locator('#price').fill(price);await page.locator('#order').click();await until('order receipt',async()=>await page.locator('#receipts p').count()===previous+1&&(await page.locator('#receipts p').last().textContent()).includes(expected));}
  async function deposit(i,denom,amount,confirmed) {phase='deposit-'+i;await login(i);const before=await account();assert(before.includes(`${denom}: 지갑 1000000000000 / 예치 C 0 atoms`));await page.locator('#asset').selectOption(denom);await page.locator('#amount').fill(amount);await page.locator('#deposit').click();await until('TX submitted',async()=>(await text('txs')).includes('SUBMISSION_UNKNOWN'));await until('TX committed',async()=>{const b=page.locator('#txs button');if(await b.count())await b.first().click();return (await text('txs')).includes('COMMITTED');});const tx=await text('txs');assert.match(tx,/COMMITTED · [0-9a-fA-F]{64} · 높이 [1-9][0-9]*/);const after=await account();assert(after.includes(`${denom}: 지갑 ${1000000000000n-BigInt(confirmed)} / 예치 C ${confirmed} atoms`));evidence.txs.push({account:i,denom,before,after,receipt:tx});}
  async function snapshot(i) {await login(i);return {ledger:await ledger(),orders:await text('orders'),fills:await text('fills'),chain:await account()};}
  try {
    await control('temporary_start');await page.goto('http://127.0.0.1:5173');page.on('framenavigated',f=>{if(f===page.mainFrame())navigations++;});
    await page.locator('#create').click();const publicKeys=JSON.parse(await text('public'));assert.equal(publicKeys.length,2);assert.notEqual(publicKeys[0],publicKeys[1]);await writeFile(path.join(output,'public-keys.json'),JSON.stringify(publicKeys,null,2));
    const pins=await control('init',{publicKeys});evidence.genesis=pins.chain_genesis;
    evidence.sourceHashes={};for(const f of ['protocol/s2/CONTRACT.md','protocol/s2/profile.json','exchange/Cargo.lock','web/package-lock.json']){try{evidence.sourceHashes[f]=createHash('sha256').update(await readFile(f)).digest('hex');}catch{throw Error('missing source '+f);}}
    evidence.runtimePins=pins;await control('temporary_stop');evidence.firstHealth=await control('start');await page.locator('#genesis').fill(pins.chain_genesis);await page.locator('#bind').click();
    await deposit(0,'DEVBASE','10','10000000');await check('A actual deposit',expectA(0,0,0,10000000));
    await deposit(1,'DEVQUOTE','100','100000000');await check('B actual deposit',expectB(0,0,100000000));
    phase='gtc';await login(0);await order('SELL','GTC','2','10');await check('A reserves two BASE',expectA(2000000,0,0,8000000));assert((await text('book')).includes('10000 / 2000'));
    await login(1);await order('BUY','GTC','1','10');await check('B first pending fill',expectB(10000000,1000000,90000000));
    await login(0);await check('A partial one BASE',expectA(1000000,1000000,10000000,8000000));assert((await text('book')).includes('10000 / 1000'));assert((await text('fills')).includes('잠정 · 1000 lots @ 10000 ticks'));
    phase='cancel';await page.getByRole('button',{name:'잔량 취소',exact:true}).click();await check('A cancellation releases R only',expectA(0,1000000,10000000,9000000));assert((await text('orders')).includes('CANCELLED_OFFCHAIN'));
    phase='ioc';await order('SELL','GTC','0.5','10');await login(1);await order('BUY','IOC','1','10.001');await check('B IOC worst limit held',expectB(15000500,1500000,84999500));assert((await text('orders')).includes('LIMIT_IOC'));assert((await text('fills')).includes('잠정 · 500 lots @ 10000 ticks'));
    await login(0);await check('A cumulative pending',expectA(0,1500000,15000000,8500000));
    phase='P-unusable';await login(1);await order('SELL','GTC','0.001','10','REJECTED');await check('B cannot reserve pending BASE',expectB(15000500,1500000,84999500));
    phase='withdraw-hold';const txsBefore=posts;await page.locator('#asset').selectOption('DEVBASE');await page.locator('#amount').fill('0.001');await page.locator('#withdraw').click();await until('UNSETTLED_HOLD',async()=>(await text('status')).includes('정산 미구현/미정산 보류'));await Promise.all(responses);assert.equal(posts,txsBefore);assert.equal(posts,2);assert(evidence.receipts.some(r=>r.path==='/s2/me/withdraw-prepare'&&r.body.code==='UNSETTLED_HOLD'));evidence.withdraw={txPostsBefore:txsBefore,txPostsAfter:posts,status:await text('status')};
    await page.locator('#abort-withdraw').click();await until('abort receipt',async()=>{await Promise.all(responses);return evidence.receipts.some(r=>r.path==='/s2/me/withdraw-abort');});
    const abort=evidence.receipts.find(r=>r.path==='/s2/me/withdraw-abort');
    if(abort.body.code==='STALE') {
      // Guide troubleshooting: inspect observed height/health before retrying a final rejection.
      evidence.abortStale={receipt:abort,health:await control('health')};
      await until('next finalized observation',async()=>{const m=(await text('freshness')).match(/높이 (\d+)/);return m&&BigInt(m[1])>BigInt(abort.body.observed_height);});
      await page.locator('#abort-withdraw').click();
    }
    await until('abort withdrawal',async()=>await text('status')==='출금 준비 동결 해제');
    phase='before-restart';evidence.before=[await snapshot(0),await snapshot(1)];await page.screenshot({path:path.join(output,'first.png')});evidence.firstStop=await control('stop');
    phase='restart';evidence.restartHealth=await control('start');evidence.after=[await snapshot(0),await snapshot(1)];
    for(let i=0;i<2;i++){for(const k of ['ledger','orders','fills'])assert.deepEqual(evidence.after[i][k],evidence.before[i][k],`restart account ${i} ${k}`);assert.equal(evidence.before[i].chain.split(' · ')[0],evidence.after[i].chain.split(' · ')[0]);}
    assert.deepEqual(JSON.parse(await text('public')),publicKeys);assert.equal(await page.locator('#genesis').inputValue(),pins.chain_genesis);assert.equal(navigations,0);await page.screenshot({path:path.join(output,'restarted.png')});evidence.secondStop=await control('stop');
    await Promise.all(responses);for(const r of evidence.receipts.filter(r=>['/s2/orders','/s2/cancels'].includes(r.path))){assert.equal(r.body.durability,'LOCAL_FSYNC');assert.equal(r.body.replicated,false);}
    evidence.samePage=true;evidence.navigationsAfterKeyCreation=navigations;evidence.txPosts=posts;evidence.result='PASS';return {scope:evidence.scope,documentQA:'PASS',samePage:true,navigationsAfterKeyCreation:navigations};
  } catch(e) {evidence.failure={phase,error:String(e),status:await text('status').catch(()=>null)};throw e;}
  finally {await Promise.all(responses);evidence.finished=new Date().toISOString();await writeFile(path.join(output,'qa-guide.json'),JSON.stringify(evidence,null,2)+'\n');}
}
