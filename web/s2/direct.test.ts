import test from 'node:test';
import assert from 'node:assert/strict';
import { TradingKey, context, base64 } from './session.ts';
import { DirectClient, validateAccount, type Account } from './direct.ts';
const ctx=context('ab'.repeat(32));
function account(key: TradingKey): Account {return {state:'COMMITTED',signing_ready:true,context:ctx,owner:key.address,owner_base64:key.owner,public_key_type:'/cosmos.crypto.mldsa65.PubKey',public_key_base64:base64.encode(key.publicKey),account_number:'1',sequence:'0',owner_epoch:'0',balances:['DEVBASE','DEVQUOTE'].map(denom=>({denom,bank_atoms:'100000000',confirmed_atoms:'0'})),gas_denom:'DEVGAS',gas_atoms:'100000',observed_height:'12',cursor_height:'12',block_hash:'11'.repeat(32),block_time_unix_ms:String(Date.now()),freshness_ms:'0',query_latency_ms:'0',indexer_mode:'DIRECT_COMMITTED_QUERY'};}
test('DIRECT rejects mixed context, key, height, stale and noncanonical amounts',()=>{
 const key=new TradingKey();try {validateAccount(account(key),key,ctx,Date.now());
 for(const change of [{owner:'foreign'},{owner_base64:'foreign'},{signing_ready:false},{cursor_height:'13'},{public_key_base64:''},{gas_denom:'DEVQUOTE'},{sequence:'01'},{sequence:'18446744073709551616'},{freshness_ms:'5001'},{query_latency_ms:'2001'},{block_time_unix_ms:String(Date.now()-6000)},{context:{...ctx,genesis_hash:'cd'.repeat(32)}},{balances:[{denom:'DEVBASE',bank_atoms:'1e6',confirmed_atoms:'0'},account(key).balances[1]]}])assert.throws(()=>validateAccount({...account(key),...change} as Account,key,ctx,Date.now()));
 }finally{key.destroy();}
});
test('DIRECT lost response locks new IDs; receipt 404/foreign hash stays unknown; committed permits new TX',async()=>{
 const key=new TradingKey();let posts=0,receipt:unknown={};const api:typeof fetch=async(path)=>String(path).startsWith('/s2/accounts/')?new Response(JSON.stringify(account(key))):String(path)==='/s1/txs'?(posts++,Promise.reject(Error('lost'))):new Response(JSON.stringify(receipt));
 const c=new DirectClient(ctx,api);try {const e=await c.submit(key,'DEPOSIT','DEVBASE','1000000',()=>true);assert.equal(posts,1);await assert.rejects(()=>c.submit(key,'DEPOSIT','DEVQUOTE','1',()=>true),/UNKNOWN/);await c.resolve(e);assert.equal(e.state,'SUBMISSION_UNKNOWN');receipt={state:'COMMITTED',tx_hash:'foreign',height:'12',code:'0'};await c.resolve(e);assert.equal(e.state,'SUBMISSION_UNKNOWN');receipt={state:'COMMITTED',tx_hash:e.tx_hash,height:'12',code:'0'};await c.resolve(e);assert.equal(e.state,'COMMITTED');await c.submit(key,'DEPOSIT','DEVQUOTE','1',()=>true);assert.equal(posts,2);}finally{key.destroy();}
});
test('account switch before fetch completes signs and transmits nothing',async()=>{
 const key=new TradingKey();let posts=0;const c=new DirectClient(ctx,async path=>{if(String(path)==='/s1/txs')posts++;return new Response(JSON.stringify(account(key)));});try{await assert.rejects(()=>c.submit(key,'DEPOSIT','DEVBASE','1',()=>false),/ACCOUNT_CHANGED/);assert.equal(posts,0);assert.equal(c.history.length,0);}finally{key.destroy();}
});
test('same owner concurrent DIRECT is locked before await and height cannot regress',async()=>{
 const key=new TradingKey();let release: (r:Response)=>void=()=>{};const c=new DirectClient(ctx,()=>new Promise(r=>{release=r;}));try{const first=c.submit(key,'DEPOSIT','DEVBASE','1',()=>false);await assert.rejects(()=>c.submit(key,'DEPOSIT','DEVBASE','1',()=>true),/UNKNOWN/);release(new Response(JSON.stringify(account(key))));await assert.rejects(()=>first,/ACCOUNT_CHANGED/);const later=c.account(key);release(new Response(JSON.stringify({...account(key),observed_height:'11',cursor_height:'11'})));await assert.rejects(()=>later,/HEIGHT_REGRESSION/);}finally{key.destroy();}
});
