import { canonical } from '../s2/state.ts';
import { base64, integer } from './direct-codec.ts';
import { context, PUBLIC_RECEIPT_VERSION, type Context } from './state.ts';

export const RECEIPT_CAP = 16_777_216;
const ARRAY_CAP = 250_406;
const HASH = /^[0-9a-f]{64}$/;
const KINDS = new Set(['ORDER','CANCEL','SNAPSHOT','EXPIRY','WITHDRAW_PREPARE','WITHDRAW_ABORT','CORRECTION','SEAL_BATCH','ATTEMPT','RESOLVE_ATTEMPT','SETTLEMENT_APPLY','VOID_BATCH']);
const CODES = new Set(['OK','WITHDRAW_FROZEN','EPOCH_MISMATCH','ORDER_REVOKED','EXPIRED','EXPIRY_MARGIN','MARKET_LIMIT','FEE_CAP','FEE_GE_RECEIVE','STALE','CATCHING_UP','SNAPSHOT_CONFLICT','INSUFFICIENT_AVAILABLE','OPEN_ORDER_LIMIT','ORDER_NOT_FOUND','ID_CONFLICT','UNSETTLED_HOLD','WITHDRAW_NOT_PREPARED']);
const LISTS = ['affected_order_hashes','created_fill_ids','corrected_fill_ids','committed_fill_ids','applied_batch_ids'] as const;
const TOP = ['envelope_version','profile_id','context','principal','development_receipt','durable_ack','storage_assurance','source','account_result'];
const SOURCE = ['command_seq','record_hash','command_result_hash','after_state_hash'];
const RESULT = ['kind','request_hash','code','state','observed_height','snapshot_id',...LISTS,'ledger_changes'];
const ROW = ['denom','C','R','D','P','A'];
const CHANGE = ['owner','before','after'];

function exactObject(value: unknown, keys: readonly string[]) {
  if(value===null||Array.isArray(value)||typeof value!=='object')throw Error('RECEIPT_SCHEMA');
  const actual=Object.keys(value as object).sort(),expected=[...keys].sort();
  if(actual.length!==expected.length||actual.some((k,i)=>k!==expected[i]))throw Error('RECEIPT_SCHEMA');
  return value as Record<string,unknown>;
}
function hash(value: unknown) {if(typeof value!=='string'||!HASH.test(value))throw Error('RECEIPT_SCHEMA');return value;}
function owner(value: unknown) {
  if(typeof value!=='string'||value.length!==28)throw Error('RECEIPT_SCHEMA');
  try {const raw=base64.decode(value);if(raw.length!==20||base64.encode(raw)!==value)throw Error();}
  catch {throw Error('RECEIPT_SCHEMA');}
  return value;
}
function uint(value: unknown,bits=64) {try{return integer(value as string,(1n<<BigInt(bits))-1n);}catch{throw Error('RECEIPT_SCHEMA');}}
function hashes(value: unknown) {
  if(!Array.isArray(value)||value.length>ARRAY_CAP)throw Error('RECEIPT_SCHEMA');
  const seen=new Set<string>();
  for(const item of value){const id=hash(item);if(seen.has(id))throw Error('RECEIPT_SCHEMA');seen.add(id);}
  return value as string[];
}
function ledger(value: unknown) {
  const row=exactObject(value,ROW);
  if(!['DEVBASE','DEVQUOTE'].includes(row.denom as string))throw Error('RECEIPT_SCHEMA');
  const c=uint(row.C,128),r=uint(row.R,128),d=uint(row.D,128),p=uint(row.P,128),a=uint(row.A,128);
  if(c-r-d!==a||a<0n||p<0n)throw Error('RECEIPT_SCHEMA');
  return row;
}

// JSON.parse discards duplicate keys, so inspect the grammar before parsing.
// Canonical byte equality below then rejects whitespace, numbers, alternate
// escaping, non-ASCII text and every other old/new representation.
function uniqueKeys(raw:string) {
  let at=0,depth=0;
  const fail=():never=>{throw Error('RECEIPT_CANONICAL');};
  const string=():string=>{
    if(raw[at]!=='"')fail();const start=at++;
    for(let escaped=false;at<raw.length;at++){
      const c=raw[at];
      if(escaped){escaped=false;continue;}
      if(c==='\\'){escaped=true;continue;}
      if(c==='"'){at++;try{return JSON.parse(raw.slice(start,at)) as string;}catch{fail();}}
      if(c<' ')fail();
    }
    throw Error('RECEIPT_CANONICAL');
  };
  const literal=()=>{
    const match=/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?|^(?:true|false|null)/.exec(raw.slice(at));
    if(match===null)throw Error('RECEIPT_CANONICAL');at+=match[0].length;
  };
  const value=():void=>{
    const c=raw[at];
    if(c==='"'){string();return;}
    if(c==='['){if(++depth>8)throw Error('RECEIPT_LIMIT');at++;if(raw[at]===']'){at++;depth--;return;}for(;;){value();if(raw[at]===']'){at++;depth--;return;}if(raw[at++]!==',')fail();}}
    if(c==='{'){
      if(++depth>8)throw Error('RECEIPT_LIMIT');at++;const seen=new Set<string>();
      if(raw[at]==='}'){at++;depth--;return;}
      for(;;){const key=string();if(seen.has(key))fail();seen.add(key);if(raw[at++]!==':')fail();value();if(raw[at]==='}'){at++;depth--;return;}if(raw[at++]!==',')fail();}
    }
    literal();
  };
  value();if(at!==raw.length)fail();
}

export interface PublicReceipt {
  envelope_version:string;profile_id:string;context:Context;principal:string;
  development_receipt:string;durable_ack:false;storage_assurance:string;
  source:{command_seq:string;record_hash:string;command_result_hash:string;after_state_hash:string};
  account_result:{kind:string;request_hash:string;code:string;state:'LOCAL_ACCEPTED'|'REJECTED';observed_height:string;snapshot_id:string;affected_order_hashes:string[];created_fill_ids:string[];corrected_fill_ids:string[];committed_fill_ids:string[];applied_batch_ids:string[];ledger_changes:Array<{owner:string;before:Record<string,unknown>;after:Record<string,unknown>}>};
}

export function decodePublicReceipt(bytes:Uint8Array,expectedContext:Context,principal:string):PublicReceipt {
  if(bytes.length>RECEIPT_CAP)throw Error('RECEIPT_LIMIT');
  let raw:string;
  try {raw=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(bytes);}catch{throw Error('RECEIPT_CANONICAL');}
  if([...raw].some(c=>c.charCodeAt(0)>127))throw Error('RECEIPT_CANONICAL');
  uniqueKeys(raw);
  let value:unknown;try{value=JSON.parse(raw);}catch{throw Error('RECEIPT_CANONICAL');}
  const rejectNumbers=(v:unknown):void=>{if(typeof v==='number')throw Error('RECEIPT_CANONICAL');if(Array.isArray(v))for(const x of v)rejectNumbers(x);else if(v&&typeof v==='object')for(const x of Object.values(v))rejectNumbers(x);};
  rejectNumbers(value);
  if(canonical(value)!==raw)throw Error('RECEIPT_CANONICAL');
  const top=exactObject(value,TOP);
  if(top.envelope_version!==PUBLIC_RECEIPT_VERSION||top.profile_id!=='s3-dev-local-v1'||top.development_receipt!=='LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE'||top.durable_ack!==false||top.storage_assurance!=='UNPROVEN_HOST_SPACE')throw Error('RECEIPT_SCHEMA');
  try{context(top.context as Context,expectedContext);}catch{throw Error('RECEIPT_CONTEXT');}if(owner(top.principal)!==principal)throw Error('RECEIPT_PRINCIPAL');
  const source=exactObject(top.source,SOURCE);if(uint(source.command_seq)===0n)throw Error('RECEIPT_SCHEMA');
  hash(source.record_hash);hash(source.command_result_hash);hash(source.after_state_hash);
  const result=exactObject(top.account_result,RESULT);
  if(!KINDS.has(result.kind as string)||!CODES.has(result.code as string)||!['LOCAL_ACCEPTED','REJECTED'].includes(result.state as string))throw Error('RECEIPT_SCHEMA');
  if((result.code==='OK')!==(result.state==='LOCAL_ACCEPTED'))throw Error('RECEIPT_SCHEMA');
  hash(result.request_hash);uint(result.observed_height);hash(result.snapshot_id);
  for(const name of LISTS)hashes(result[name]);
  if(!Array.isArray(result.ledger_changes)||result.ledger_changes.length>2)throw Error('RECEIPT_SCHEMA');
  let last='';
  for(const item of result.ledger_changes){const change=exactObject(item,CHANGE);if(owner(change.owner)!==principal)throw Error('RECEIPT_PRINCIPAL');const before=ledger(change.before),after=ledger(change.after);if(before.denom!==after.denom||last>=String(after.denom))throw Error('RECEIPT_SCHEMA');last=String(after.denom);}
  return value as PublicReceipt;
}

export interface ReceiptEvidence {key:string;raw:Uint8Array;source:PublicReceipt['source'];receipt:PublicReceipt}
export interface ReceiptConflict {saved:ReceiptEvidence;received:ReceiptEvidence}
export class ReceiptLedger {
  readonly entries:ReceiptEvidence[]=[];readonly conflicts:ReceiptConflict[]=[];
  readonly gaps=new Set<string>();
  gapFor(ctx:Context,principal:string) {return this.gaps.has(canonical([ctx,principal,PUBLIC_RECEIPT_VERSION]));}
  accept(bytes:Uint8Array,ctx:Context,principal:string) {
    const receipt=decodePublicReceipt(bytes,ctx,principal),seq=receipt.source.command_seq;
    const key=canonical([ctx,principal,receipt.envelope_version,seq]);
    const evidence:ReceiptEvidence={key,raw:bytes.slice(),source:structuredClone(receipt.source),receipt};
    const saved=this.entries.find(entry=>entry.key===key);
    if(saved) {
      const same=saved.raw.length===evidence.raw.length&&saved.raw.every((b,i)=>b===evidence.raw[i])&&canonical(saved.source)===canonical(evidence.source);
      if(!same){this.conflicts.push({saved,received:evidence});throw Error('CLIENT_RECEIPT_MISMATCH');}
      return saved.receipt;
    }
    const principalEntries=this.entries.filter(entry=>entry.receipt.principal===principal&&canonical(entry.receipt.context)===canonical(ctx));
    if(principalEntries.some(entry=>uint(entry.receipt.source.command_seq)+1n<uint(seq)))this.gaps.add(canonical([ctx,principal,PUBLIC_RECEIPT_VERSION]));
    this.entries.push(evidence);return receipt;
  }
}
