import test from 'node:test';
import assert from 'node:assert/strict';
import {attachPage,parseContext} from './page.ts';
const valid=JSON.stringify({chain_id:'nus-s3-dev-1',service_schema:'s3/3',genesis_hash:'a'.repeat(64),contract_hash:'b'.repeat(64),config_hash:'c'.repeat(64),market_id:'DEVBASE/DEVQUOTE',market_config_version:'1'});
function fixture(transport:typeof fetch){
  const callbacks:Record<string,Array<()=>unknown>>={};
  const win:any={location:{origin:'http://127.0.0.1:5173'},addEventListener(n:string,f:()=>unknown){(callbacks[n]??=[]).push(f);},removeEventListener(){}};
  const doc:any={defaultView:win};
  const node=():any=>({ownerDocument:doc,checked:false,disabled:false,textContent:'',children:[],
    events:{} as Record<string,()=>unknown>,addEventListener(n:string,f:()=>unknown){this.events[n]=f;},
    append(...v:any[]){this.children.push(...v);},replaceChildren(...v:any[]){this.children=v;},setAttribute(){}});
  const nodes:any=Object.fromEntries(['enabled','acknowledge','prepare','activate','registration','status','wallet'].map(k=>[k,node()]));
  nodes.activate.disabled=true;doc.getElementById=(id:string)=>nodes[id];doc.createElement=node;
  const page=attachPage(doc,transport);return {nodes,callbacks,page};
}
test('context parser has bounded public string shape and exact profile',()=>{
  assert.equal(parseContext(valid).chain_id,'nus-s3-dev-1');
  for(const raw of ['[]','null','{}','{"chain_id":"other","service_schema":"s3/3"}','{"chain_id":"nus-s3-dev-1","service_schema":"s3/3","__proto__":"x"}',' '.repeat(16385)])assert.throws(()=>parseContext(raw));
});
test('page is inert, two opt-ins precede keys, only public registration leaves tab',()=>{
  let io=0;const f=fixture((async()=>{io++;throw Error('unexpected');}) as typeof fetch);
  try {
    assert.equal(f.nodes.registration.textContent,'');f.nodes.prepare.events.click();assert.equal(f.nodes.registration.textContent,'');
    f.nodes.enabled.checked=true;f.nodes.acknowledge.checked=true;f.nodes.prepare.events.click();
    const rows=JSON.parse(f.nodes.registration.textContent);assert.equal(rows.length,2);assert.deepEqual(Object.keys(rows[0]).sort(),['address','owner','public_key_base64']);
    const first=f.nodes.registration.textContent;f.nodes.prepare.events.click();assert.equal(f.nodes.registration.textContent,first);assert.equal(io,0);
  }finally{f.page.destroy();}assert.equal(f.nodes.registration.textContent,'');
});
test('explicit context fetch mounts once without login and pagehide clears keys',async()=>{
  let io=0;const f=fixture((async(url,options)=>{io++;assert.equal(url,'/runtime-context.json');assert.equal(options?.redirect,'error');return new Response(valid,{headers:{'content-type':'application/json'}});}) as typeof fetch);
  try {
    f.nodes.enabled.checked=f.nodes.acknowledge.checked=true;f.nodes.prepare.events.click();
    await f.nodes.activate.events.click();assert.equal(io,1);assert.ok(f.nodes.wallet.children.length>0);
    await f.nodes.activate.events.click();assert.equal(io,1);
    for(const fn of f.callbacks.pagehide)fn();assert.equal(f.nodes.registration.textContent,'');assert.equal(f.nodes.wallet.children.length,0);
  }finally{f.page.destroy();}
});
test('oversize, redirected failure, invalid context and late completion never mount',async()=>{
  for(const kind of ['large','invalid','error','late']){
    let f:ReturnType<typeof fixture>;f=fixture((async()=>{
      if(kind==='error')throw Error('private diagnostic');
      if(kind==='late')f.page.destroy();
      return new Response(kind==='large'?'x'.repeat(16385):kind==='invalid'?'{}':valid,{headers:{'content-type':'application/json'}});
    }) as typeof fetch);
    try{f.nodes.enabled.checked=f.nodes.acknowledge.checked=true;f.nodes.prepare.events.click();await f.nodes.activate.events.click();assert.equal(f.nodes.wallet.children.length,0);assert.ok(!f.nodes.status.textContent.includes('private diagnostic'));}finally{f.page.destroy();}
  }
});
