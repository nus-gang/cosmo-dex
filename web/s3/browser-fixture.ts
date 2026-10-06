// TEST ONLY: synthetic transport, never imported by production build.
import { mount } from './component.ts';
import { LocalClient } from './client.ts';
import { LocalKey } from './key.ts';
import { encode } from '../src/codec.ts';
import { base64 } from './direct-codec.ts';
import raw from './fixtures/ld-rest.json' with {type:'json'};
const keys=[new LocalKey(),new LocalKey()],ctx=raw.owner_projection.context;
let selected=0,posts=0,mode='ready',revision=1;
let accountReply: (()=>Promise<Response>)|undefined;
const transport: typeof fetch=async(path,init)=>{
  let data:any;
  if(String(path).endsWith('auth/challenge')) {
    selected=keys.findIndex(k=>k.owner===JSON.parse(init!.body as string).owner);
    const now=Math.floor(Date.now()/1000);
    data={wire_base64:base64.encode(encode('WalletChallengeV1',{protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,owner:keys[selected].owner,server_origin:'http://127.0.0.1:5173',audience:'exchange-api',challenge_nonce:'aa'.repeat(32),issued_at:String(now),expiry_time:String(now+100)}))};
  }else if(String(path).endsWith('auth/session')) data={token:'fixture'};
  else if(String(path).endsWith('capabilities'))data={...raw.receipt,api_prefix:'/dev-local/v1/',signed_result_query:true,automatic_withdraw:false,ws:false};
  else if(String(path).endsWith('account')) {
    if(accountReply)return accountReply();
    data={...structuredClone(raw.other_projection),revision:String(revision),owner:keys[selected].owner,received_at_unix_ms:String(Date.now()),withdraw_frozen:true,withdraw_ready:true};
    if(mode==='held'){data.ledger[0].P='1000000';data.withdraw_ready=false;data.fills=[{fill_id:'fixture-pending',state:'SUBMISSION_UNKNOWN',revision:'1'}];}
  }else data=raw.receipt;
  return new Response(JSON.stringify(data));
};
const client=new LocalClient(ctx,transport,{
  account:async()=>({context:ctx,owner:keys[selected].address,public_key_base64:base64.encode(keys[selected].publicKey),account_number:'1',sequence:'0',owner_epoch:'0',observed_height:'100',received_at_unix_ms:String(Date.now()),gas_atoms:'1000'}),
  broadcast:async()=>{posts++;throw Error('lost');},result:async()=>{throw Error('NOT_FOUND');},
},true,true);
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
Object.assign(globalThis,{fixture:{client,component,reorder,posts:()=>posts,held:()=>{mode='held';selected=keys.findIndex(k=>k.owner===client.projection.owner);client.select(keys[selected]);},owners:keys.map(k=>k.owner)}});
