import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { canonical } from '../s2/state.ts';
import { decodePublicReceipt, ReceiptLedger, RECEIPT_CAP } from './receipt.ts';
import type { Context } from './state.ts';

const root=new URL('../../proposals/s3-local-account-receipt-v1/vectors/',import.meta.url);
const read=(name:string)=>new Uint8Array(readFileSync(new URL(name,root)));
const index=JSON.parse(readFileSync(new URL('index.json',root),'utf8')) as {cases:Array<{id:string;expected:string;principal:string;context:Context}>};
const negatives=JSON.parse(readFileSync(new URL('negative-index.json',root),'utf8')) as Array<{id:string;file:string;error:string}>;
const negativeBaseline=index.cases.find(item=>item.id==='fee0-taker-trade')!;

for(const item of index.cases)test(`approved public receipt vector ${item.id}`,()=>{
  const value=decodePublicReceipt(read(item.expected),item.context,item.principal);
  assert.equal(value.source.command_seq,JSON.parse(readFileSync(new URL(item.expected,root),'utf8')).source.command_seq);
});
for(const item of negatives)test(`strict public receipt rejects ${item.id}`,()=>{
  assert.throws(()=>decodePublicReceipt(read(item.file),negativeBaseline.context,negativeBaseline.principal),new RegExp(item.error));
});
test('body cap and nesting cap fail before schema interpretation',()=>{
  assert.throws(()=>decodePublicReceipt(new Uint8Array(RECEIPT_CAP+1),index.cases[0].context,index.cases[0].principal),/RECEIPT_LIMIT/);
  assert.throws(()=>decodePublicReceipt(new TextEncoder().encode('[[[[[[[[[]]]]]]]]]'),index.cases[0].context,index.cases[0].principal),/RECEIPT_LIMIT/);
});
test('ledger preserves both byte sources and closes a same full-key collision',()=>{
  const item=index.cases[0],raw=read(item.expected),ledger=new ReceiptLedger();
  const first=ledger.accept(raw,item.context,item.principal);assert.equal(ledger.accept(raw,item.context,item.principal),first);assert.equal(ledger.entries.length,1);
  const changed=structuredClone(first);changed.account_result.request_hash='ff'.repeat(32);
  const other=new TextEncoder().encode(canonical(changed));
  assert.throws(()=>ledger.accept(other,item.context,item.principal),/CLIENT_RECEIPT_MISMATCH/);
  assert.equal(ledger.conflicts.length,1);assert.deepEqual(ledger.conflicts[0].saved.raw,raw);assert.deepEqual(ledger.conflicts[0].received.raw,other);
});
test('receipt sequence gap is tracked separately and does not synthesize missing effects',()=>{
  const first=index.cases[0],later=index.cases.find(v=>BigInt(JSON.parse(readFileSync(new URL(v.expected,root),'utf8')).source.command_seq)>2n)!;
  const ledger=new ReceiptLedger();ledger.accept(read(first.expected),first.context,first.principal);
  // Use exact same account/context with a deliberately later, otherwise valid source.
  const value=JSON.parse(readFileSync(new URL(first.expected,root),'utf8'));value.source.command_seq='3';value.account_result.request_hash='33'.repeat(32);
  ledger.accept(new TextEncoder().encode(canonical(value)),first.context,first.principal);
  assert.equal(ledger.gapFor(first.context,first.principal),true);assert.equal(ledger.entries.length,2);assert.ok(later);
});
test('consecutive and later backfilled receipt sequences do not retain a false history gap',()=>{
  const first=index.cases[0],base=JSON.parse(readFileSync(new URL(first.expected,root),'utf8'));
  const bytes=(seq:string,request:string)=>{const value=structuredClone(base);value.source.command_seq=seq;value.account_result.request_hash=request;return new TextEncoder().encode(canonical(value));};
  const consecutive=new ReceiptLedger();
  for(const seq of ['1','2','3'])consecutive.accept(bytes(seq,seq.repeat(64)),first.context,first.principal);
  assert.equal(consecutive.gapFor(first.context,first.principal),false);
  const backfilled=new ReceiptLedger();backfilled.accept(bytes('1','11'.repeat(32)),first.context,first.principal);backfilled.accept(bytes('3','33'.repeat(32)),first.context,first.principal);
  assert.equal(backfilled.gapFor(first.context,first.principal),true);backfilled.accept(bytes('2','22'.repeat(32)),first.context,first.principal);
  assert.equal(backfilled.gapFor(first.context,first.principal),false);
});
test('query sequence and request identity remain bound to their first receipt source',()=>{
  const first=index.cases[0],base=JSON.parse(readFileSync(new URL(first.expected,root),'utf8'));
  const bytes=(seq:string,source='11'.repeat(32))=>{const value=structuredClone(base);value.source.command_seq=seq;value.source.record_hash=source;return new TextEncoder().encode(canonical(value));};
  const query=new ReceiptLedger();assert.throws(()=>query.accept(bytes('8'),first.context,first.principal,'7'),/CLIENT_RECEIPT_MISMATCH/);
  assert.equal(query.entries.length,0);assert.equal(query.queryMismatches.length,1);assert.equal(query.queryMismatches[0].requestedCommandSeq,'7');assert.equal(query.queryMismatches[0].received.source.command_seq,'8');
  const request=new ReceiptLedger();request.accept(bytes('7'),first.context,first.principal,'7');
  assert.throws(()=>request.accept(bytes('8','99'.repeat(32)),first.context,first.principal,'8'),/CLIENT_RECEIPT_MISMATCH/);
  assert.equal(request.entries.length,1);assert.equal(request.conflicts.length,1);assert.equal(request.conflicts[0].saved.source.command_seq,'7');assert.equal(request.conflicts[0].received.source.command_seq,'8');
});
