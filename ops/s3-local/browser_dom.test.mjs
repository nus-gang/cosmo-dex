// Actual approved Wallet mount + actual supervisor expressions. No network/keys.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mount} from '../../web/s3/component.ts';
import {uiExpression} from './browser_supervisor.mjs';
const require=createRequire('/Users/gangdongju/.paperclip/cli/current/node_modules/paperclipai/package.json');
const {JSDOM}=require('jsdom');
function fixture(){
  const dom=new JSDOM('<main id="wallet"></main>',{url:'http://127.0.0.1:5173/',runScripts:'outside-only'});
  dom.window.TextEncoder=TextEncoder;
  const calls=[],hash='a'.repeat(64),oid='b'.repeat(64);
  const client={projection:{owner:'public-a',generation:0,reason:'',open:()=>true,
    view:{revision:'1',ledger:[{denom:'DEVBASE',C:'10000000',R:'0',D:'0',P:'0',A:'10000000'}],
      orders:[{order_id:oid,order_hash:hash,side:'SELL',state:'OPEN',remaining_qty_lots:'2',limit_price_ticks:'10'}],fills:[],batches:[]}},
    history:[],commands:[],receipt:'PUBLIC RECEIPT',publicReceipts:{conflicts:[],queryMismatches:[]},
    canWithdraw:()=>true,canDeposit:()=>true,canTrade:()=>true,select(k){this.projection.owner=k.owner;calls.push(['select',k.owner]);},destroy(){}};
  for(const name of ['login','refresh','deposit','order','cancel','querySigned','prepare','withdraw','resolve'])client[name]=async(...args)=>{calls.push([name,...args]);};
  const keys=['public-a','public-b'].map(owner=>({owner,address:owner,destroy(){}}));
  const mounted=mount(dom.window.document.getElementById('wallet'),client,keys,'http://127.0.0.1:5173');
  return {dom,client,calls,hash,oid,act:(action,a)=>dom.window.eval(uiExpression(action,a)),close:()=>{mounted.destroy();dom.window.close();}};
}
test('approved aria DOM supports account, deposit, GTC/IOC, cancel and withdrawal inputs',async()=>{
  const f=fixture();try{
    assert.equal(f.dom.window.document.querySelectorAll('label').length,0);
    f.act('select',{name:'계정',value:'1'});
    f.act('select',{name:'예치 자산',value:'DEVQUOTE'});f.act('input',{name:'예치 금액',value:'100'});f.act('click',{name:'직접 서명하여 예치'});
    f.act('select',{name:'주문 방향',value:'BUY'});f.act('select',{name:'주문 유형',value:'IOC'});
    f.act('select',{name:'최대 수수료 bps',value:'25'});f.act('input',{name:'주문 수량 lots',value:'1'});f.act('input',{name:'지정가 ticks',value:'12'});f.act('click',{name:'주문 서명·제출'});
    f.act('select',{name:'취소 주문',value:f.oid+':'+f.hash});f.act('click',{name:'잔량 취소 서명·제출'});
    f.act('select',{name:'출금 자산',value:'DEVBASE'});f.act('amount',{value:'0.9975'});f.act('click',{name:'직접 서명하여 출금'});
    await Promise.resolve();
    assert.ok(f.calls.some(c=>c[0]==='deposit'&&c[1]==='DEVQUOTE'&&c[2]==='100000000'));
    assert.deepEqual(f.calls.find(c=>c[0]==='order')[1],{side:'BUY',order_type:'IOC',max_qty_lots:'1',limit_price_ticks:'12',max_fee_bps:'25'});
    assert.deepEqual(f.calls.find(c=>c[0]==='cancel'),['cancel',f.oid,f.hash]);
    assert.deepEqual(f.calls.find(c=>c[0]==='withdraw'),['withdraw','DEVBASE','997500']);
  }finally{f.close();}
});
test('snapshot captures public ledger request receipt state and finite choices, no client export',()=>{
  const f=fixture();try{
    const result=f.act('snapshot');assert.equal(result.schema,'s3-local-browser-screen/1');
    assert.equal(result.identity,'계정 public-a · revision 1');assert.equal(result.ledger[0][1],'10.000000');
    assert.ok(result.receipts.includes('PUBLIC RECEIPT'));assert.ok(result.orders[0].includes(f.oid));
    assert.equal(result.buttons.length,10);assert.equal(result.selections.find(s=>s.name==='취소 주문').options[0].value,f.oid+':'+f.hash);
    assert.equal(Object.keys(result).some(k=>/key|token|signature|wire/.test(k)),false);
    f.dom.window.document.querySelector('[role=status]').textContent='x'.repeat(16385);
    assert.throws(()=>f.act('snapshot'),/UI_SIZE/);
    f.dom.window.document.querySelector('[role=status]').textContent='가'.repeat(16000);
    assert.throws(()=>f.act('snapshot'),/UI_SIZE/);
  }finally{f.close();}
});
test('duplicate, disabled, missing targets, wrong enum/option and arbitrary JS fail before effect',()=>{
  const f=fixture();try{
    const before=f.calls.length;
    for(const a of [{name:'계정',value:'DEVBASE'},{name:'주문 유형',value:'MARKET'},{name:'취소 주문',value:'c'.repeat(64)+':'+f.hash}])assert.throws(()=>f.act('select',a));
    for(const value of ['1e2','01','-1','1.5',"';globalThis.key"])assert.throws(()=>f.act('input',{name:'주문 수량 lots',value}));
    assert.throws(()=>f.act('eval',{value:'secret'}));
    const b=[...f.dom.window.document.querySelectorAll('button')].find(b=>b.textContent==='직접 서명하여 예치');b.disabled=true;assert.throws(()=>f.act('click',{name:b.textContent}));
    const s=f.dom.window.document.querySelector('[aria-label="계정"]');s.after(s.cloneNode(true));assert.throws(()=>f.act('select',{name:'계정',value:'0'}));
    assert.equal(f.calls.length,before);
  }finally{f.close();}
});
