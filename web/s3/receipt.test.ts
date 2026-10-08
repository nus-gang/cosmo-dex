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
  const value=JSON.parse(readFileSync(new URL(first.expected,root),'utf8'));value.source.command_seq='3';
  ledger.accept(new TextEncoder().encode(canonical(value)),first.context,first.principal);
  assert.equal(ledger.gapFor(first.context,first.principal),true);assert.equal(ledger.entries.length,2);assert.ok(later);
});
