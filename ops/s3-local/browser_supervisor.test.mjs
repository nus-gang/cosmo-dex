import assert from 'node:assert/strict';
import test from 'node:test';
import {Lifetime} from './browser_supervisor.mjs';
function fixture(){
  let now=0,parent=true,mode='',effects=[];const registration=JSON.stringify([{public_key_base64:'public-a'},{public_key_base64:'public-b'}]);
  const context={chain_id:'mock',genesis_hash:'1'.repeat(64)};
  const life=new Lifetime({clock:()=>now,parent:()=>parent,limit:1000,emit:(p,v)=>effects.push([p,v]),request:async(action,a)=>{
    if(mode==='disconnect')throw Error('secret');
    if(action==='observe')return {nonce:mode==='nonce'?'old':a.challenge,document:mode==='navigation'?'new':a.document,origin:mode==='origin'?'https://other':'http://127.0.0.1:5173',registration:mode==='registration'?'changed':registration};
    if(action==='prepare')return {registration};
    if(action==='context')return mode==='context'?{chain_id:'other'}:context;
    effects.push(['action',action]);
  }});
  return {life,context,registration,effects,mode:x=>mode=x,time:x=>now=x,parent:x=>parent=x};
}
test('same document prepare and exact Context activation then finite close',async()=>{
  const f=fixture();await f.life.act('prepare');assert.equal(f.life.state,'KEYS_HELD');
  await f.life.act('activate',{context:f.context,registration:f.registration});assert.equal(f.life.state,'ACTIVE');
  await f.life.act('click',{name:'인증·조회'});await f.life.act('finish');assert.equal(f.life.state,'CLOSED');
  await assert.rejects(f.life.act('inspect'));const ns=f.effects.filter(x=>x[0]==='observed').map(x=>x[1].nonce);assert.equal(new Set(ns).size,ns.length);
});
test('lost pipe tab navigation origin and nonce close before any UI effect',async()=>{
  for(const mode of ['disconnect','navigation','origin','nonce']){const f=fixture();f.mode(mode);await assert.rejects(f.life.act('prepare'));assert.equal(f.life.state,'FAILED');assert.equal(f.effects.filter(x=>x[0]==='action').length,0);}
});
test('parent loss deadline and clock regression close retained-key lane',async()=>{
  for(const mode of ['parent','deadline','clock']){const f=fixture();await f.life.act('prepare');if(mode==='parent')f.parent(false);else f.time(mode==='deadline'?1000:-1);await assert.rejects(f.life.act('inspect'));assert.equal(f.life.state,'FAILED');}
});
test('registration or Context mismatch denies activation no private recovery',async()=>{
  for(const mode of ['registration','context']){const f=fixture();await f.life.act('prepare');f.mode(mode);await assert.rejects(f.life.act('activate',{context:f.context,registration:f.registration}));assert.equal(f.life.state,'FAILED');assert.equal(f.effects.filter(x=>x[0]==='action').length,0);}
});
test('no preactivation trading action duplicate key creation or arbitrary operation',async()=>{
  for(const action of ['click','select','amount','export-key','reload']){const f=fixture();await assert.rejects(f.life.act(action));assert.equal(f.life.state,'FAILED');}
  const f=fixture();await f.life.act('prepare');await assert.rejects(f.life.act('prepare'));assert.equal(f.life.state,'FAILED');
});
