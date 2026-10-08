import test from 'node:test';
import assert from 'node:assert/strict';
import { TabEntry } from './entry.ts';
const origin = 'http://127.0.0.1:5173';
test('default off and exact loopback origin', () => {
  for (const flags of [[], [true], [false,true], [true,false]])
    assert.throws(() => new TabEntry(origin,...flags), /TWO_OPT_INS_REQUIRED/);
  for (const value of ['https://127.0.0.1:5173','http://127.0.0.1:5174','http://example.com:5173',origin+'/'])
    assert.throws(() => new TabEntry(value,true,true), /ORIGIN_REJECTED/);
});
test('fresh two tab keys expose only copied public registration data', () => {
  const a=new TabEntry(origin,true,true), b=new TabEntry(origin,true,true);
  try {
    const rows=a.registrations();
    assert.equal(new Set([...rows,...b.registrations()].map(r=>r.owner)).size,4);
    assert.deepEqual(Object.keys(rows[0]).sort(),['address','owner','public_key_base64']);
    rows[0].owner='changed';assert.notEqual(a.registrations()[0].owner,'changed');
    assert.equal(Buffer.from(rows[0].public_key_base64,'base64').length,1952);
  } finally {a.destroy();b.destroy();}
  assert.throws(()=>a.registrations(),/TAB_CLOSED/);a.destroy();
});
test('context and document origin reject before transport or DOM effects',()=>{
  const a=new TabEntry(origin,true,true);let io=0;
  const transport=(async()=>{io++;throw Error('unexpected');}) as typeof fetch;
  const root={ownerDocument:{defaultView:{location:{origin:'http://external:5173'}}}} as HTMLElement;
  const ctx={chain_id:'nus-s3-dev-1',service_schema:'s3/3'};
  try {
    assert.throws(()=>a.activate(root,{...ctx,chain_id:'other'},ctx,transport),/CONTEXT_MISMATCH/);
    assert.throws(()=>a.activate(root,ctx,ctx,transport),/DOCUMENT_ORIGIN/);
    assert.equal(io,0);
  } finally {a.destroy();}
  assert.throws(()=>a.activate(root,ctx,ctx,transport),/TAB_CLOSED_OR_ACTIVE/);
});
test('approved mount is inert, single-use and pagehide closes registration',()=>{
  const a=new TabEntry(origin,true,true);let io=0;
  const events:Record<string,Array<()=>void>>={};
  const win={location:{origin},addEventListener:(name:string,fn:()=>void)=>{(events[name]??=[]).push(fn);}};
  const doc:any={defaultView:win};
  const element=(tag:string):any=>({tag,ownerDocument:doc,children:[],textContent:'',
    append(...nodes:any[]){this.children.push(...nodes);},
    replaceChildren(...nodes:any[]){this.children=nodes;},setAttribute(){},addEventListener(){}});
  doc.createElement=element;const root=element('main');
  const ctx={chain_id:'nus-s3-dev-1',service_schema:'s3/3'};
  const transport=(async()=>{io++;throw Error('unexpected');}) as typeof fetch;
  try {
    a.activate(root,ctx,ctx,transport);
    assert.ok(root.children.length>0);assert.equal(io,0);
    assert.throws(()=>a.activate(root,ctx,ctx,transport),/TAB_CLOSED_OR_ACTIVE/);
    for(const fn of events.pagehide)fn();
    assert.equal(root.children.length,0);assert.throws(()=>a.registrations(),/TAB_CLOSED/);
    assert.equal(io,0);
  }finally{a.destroy();}
});
