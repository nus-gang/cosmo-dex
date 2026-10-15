// TEST ONLY: synthetic transport, never imported by production build.
import { mount } from './component.ts';
import { LocalClient } from './client.ts';
import { LocalKey } from './key.ts';
import { decode, encode, frame } from '../src/codec.ts';
import { base64, hex } from './direct-codec.ts';
import { domains, sha256 } from '../src/wallet.ts';
import raw from './fixtures/ld-rest.json' with {type:'json'};
import { canonical } from '../s2/state.ts';
import { PREFIX, PUBLIC_RECEIPT_SCHEMA_SHA256, PUBLIC_RECEIPT_VERSION, TRUSTED_RECEIPT_VERSION } from './state.ts';
const keys=[new LocalKey(),new LocalKey()],ctx=raw.owner_projection.context;
let selected=0,posts=0,mode='ready',revision=1;
const chainRoutes:string[]=[];
const commandRoutes:string[]=[];
const orderTypes:string[]=[];
let accountReply: (()=>Promise<Response>)|undefined;
let receiptSeq=0;
let receiptMismatch=false;
const savedCommands=new Map<string,{kind:'ORDER'|'CANCEL';body:string;receipt:any}>();
let loseOrder=true;const activeOrders:any[]=[];
const publicReceipt=(request_hash:string,kind='WITHDRAW_PREPARE',command_seq=String(++receiptSeq))=>({envelope_version:PUBLIC_RECEIPT_VERSION,profile_id:'s3-dev-local-v1',context:ctx,principal:keys[selected].owner,development_receipt:'LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE',durable_ack:false,storage_assurance:'UNPROVEN_HOST_SPACE',source:{command_seq,record_hash:'11'.repeat(32),command_result_hash:'22'.repeat(32),after_state_hash:'33'.repeat(32)},account_result:{kind,request_hash,code:'OK',state:'LOCAL_ACCEPTED',observed_height:'100',snapshot_id:'55'.repeat(32),affected_order_hashes:[],created_fill_ids:[],corrected_fill_ids:[],committed_fill_ids:[],applied_batch_ids:[],ledger_changes:[]}});
const transport: typeof fetch=async(path,init)=>{
  let data:any;
  if(String(path).includes('/chain/')) {
    if(new Headers(init?.headers).get('Authorization')!=='Bearer fixture')throw Error('HTTP_401');
    chainRoutes.push(String(path));
  }
  if(String(path).endsWith('auth/challenge')) {
    selected=keys.findIndex(k=>k.owner===JSON.parse(init!.body as string).owner);
    const now=Math.floor(Date.now()/1000);
    data={wire_base64:base64.encode(encode('WalletChallengeV1',{protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,owner:keys[selected].owner,server_origin:'http://127.0.0.1:5173',audience:'exchange-api',challenge_nonce:'aa'.repeat(32),issued_at:String(now),expiry_time:String(now+100)}))};
  }else if(String(path).endsWith('auth/session')) data={token:'fixture'};
  else if(String(path).endsWith('capabilities'))data={...raw.receipt,api_prefix:'/dev-local/v1/',public_receipt_version:PUBLIC_RECEIPT_VERSION,public_receipt_schema_sha256:PUBLIC_RECEIPT_SCHEMA_SHA256,trusted_receipt_version:TRUSTED_RECEIPT_VERSION,signed_result_query:true,automatic_withdraw:false,ws:false};
  else if(String(path).endsWith('chain/account'))data={context:ctx,owner:keys[selected].address,public_key_base64:base64.encode(keys[selected].publicKey),account_number:'1',sequence:'0',owner_epoch:'0',observed_height:'100',received_at_unix_ms:String(Date.now()),gas_atoms:'1000'};
  else if(String(path).endsWith('chain/broadcast')){posts++;throw Error('lost');}
  else if(String(path).endsWith('chain/result'))throw Error('NOT_FOUND');
  else if(String(path)===PREFIX+'orders'||String(path)===PREFIX+'cancels') {
    const kind=String(path)===PREFIX+'orders'?'ORDER':'CANCEL',name=kind==='ORDER'?'OrderV1':'CancelV1',body=String(init!.body),parsed=JSON.parse(body),wire=base64.decode(parsed.wire_base64),request_hash=hex(sha256(frame(domains[name],wire)));
    commandRoutes.push(String(path));const receipt=publicReceipt(request_hash,kind);savedCommands.set(request_hash,{kind,body,receipt});
    if(kind==='ORDER'){const message=decode('OrderV1',wire);orderTypes.push(message.order_type);activeOrders.push({order_id:message.order_id,order_hash:request_hash,side:message.side==='1'?'BUY':'SELL',state:'OPEN',remaining_qty_lots:message.max_qty_lots,limit_price_ticks:message.limit_price_ticks});revision++;if(loseOrder){loseOrder=false;throw Error('lost order response');}}
    else {const message=decode('CancelV1',wire),order=activeOrders.find(o=>o.order_id===message.order_id&&o.order_hash===message.order_hash);if(order)order.remaining_qty_lots='0';revision++;}
    return new Response(canonical(receipt),{headers:{'Content-Type':'application/json','Content-Encoding':'identity'}});
  }
  else if(String(path).endsWith('receipts/orders')||String(path).endsWith('receipts/cancels')) {
    const name=String(path).endsWith('orders')?'OrderV1':'CancelV1',body=String(init!.body),wire=base64.decode(JSON.parse(body).wire_base64),request_hash=hex(sha256(frame(domains[name],wire))),saved=savedCommands.get(request_hash);
    commandRoutes.push(String(path));if(!saved||saved.body!==body)return new Response('{}',{status:404});return new Response(canonical(saved.receipt),{headers:{'Content-Type':'application/json','Content-Encoding':'identity'}});
  }
  else if(String(path).includes('receipts/commands/')) {
    const requested=String(path).split('/').at(-1)!;
    return new Response(canonical(publicReceipt('44'.repeat(32),'WITHDRAW_PREPARE',receiptMismatch?String(BigInt(requested)+1n):requested)),{headers:{'Content-Type':'application/json','Content-Encoding':'identity'}});
  }
  else if(String(path).endsWith('account')) {
    if(accountReply)return accountReply();
    data={...structuredClone(raw.other_projection),revision:String(revision),owner:keys[selected].owner,received_at_unix_ms:String(Date.now()),withdraw_frozen:true,withdraw_ready:true};
    data.orders=structuredClone(activeOrders);
    if(mode==='held'){data.ledger[0].P='1000000';data.withdraw_ready=false;data.fills=[{fill_id:'fixture-pending',state:'SUBMISSION_UNKNOWN',revision:'1'}];}
  }else {const request_id=JSON.parse(String(init!.body)).request_id,request_hash=hex(sha256(new TextEncoder().encode(canonical({request_id}))));return new Response(canonical(publicReceipt(request_hash)),{headers:{'Content-Type':'application/json','Content-Encoding':'identity'}});}
  return new Response(JSON.stringify(data));
};
const client=LocalClient.authenticated(ctx,transport,true,true);
const component=mount(document.querySelector('#app')!,client,keys,'http://127.0.0.1:5173');
async function reorder(fault: string) {
  const open=structuredClone(client.projection.view!);let release!:(r:Response)=>void;
  const pending=new Promise<Response>(r=>{release=r;});let requests=0;
  accountReply=async()=>{
    if(++requests===1)return pending;
    if(['late-recovery','late-abort','late-DP'].includes(fault))return new Response(JSON.stringify(open));
    if(fault==='503')return new Response(null,{status:503});
    if(fault==='disconnect')throw Error('DISCONNECTED');
    return new Response(JSON.stringify({...open,gate:'RECOVERY_REQUIRED',withdraw_ready:false}));
  };
  const old=client.refresh();await client.refresh();
  let response=open;
  if(fault==='late-recovery')response={...open,received_at_unix_ms:String(Date.now()),gate:'RECOVERY_REQUIRED',withdraw_ready:false};
  if(fault==='late-abort'||fault==='late-DP') {
    response={...structuredClone(open),revision:String(++revision),received_at_unix_ms:String(Date.now()),withdraw_ready:false};
    if(fault==='late-abort')response.withdraw_frozen=false;
    else {response.ledger[0].D='1';response.ledger[0].A=String(BigInt(response.ledger[0].C)-1n);response.ledger[1].P='1';}
  }
  release(new Response(JSON.stringify(response)));await old;
  if(fault==='late-abort'||fault==='late-DP')revision++;
  accountReply=undefined;component.render();
}
Object.assign(globalThis,{fixture:{client,component,reorder,chainRoutes,commandRoutes,orderTypes,posts:()=>posts,receiptMismatch:async()=>{receiptMismatch=true;let error='';try{await client.queryReceipt('7');}catch(e){error=(e as Error).message;}finally{receiptMismatch=false;component.render();}return {error,canWithdraw:client.canWithdraw(),queryMismatches:client.publicReceipts.queryMismatches.length};},held:()=>{mode='held';selected=keys.findIndex(k=>k.owner===client.projection.owner);client.select(keys[selected]);},owners:keys.map(k=>k.owner)}});
