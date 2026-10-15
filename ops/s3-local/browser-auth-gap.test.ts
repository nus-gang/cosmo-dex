import test from 'node:test';
import assert from 'node:assert/strict';
import fixture from '../../web/s3/fixtures/ld-rest.json' with {type:'json'};
import {LocalClient, type ChainPort} from '../../web/s3/client.ts';
import {LocalKey} from '../../web/s3/key.ts';
import {base64} from '../../web/s3/direct-codec.ts';
import {encode} from '../../web/src/codec.ts';
import {PREFIX} from '../../web/s3/state.ts';

test('approved client session is not available to external HTTP ChainPort', async()=>{
 const ctx=fixture.owner_projection.context, key=new LocalKey();
 const seen:{path:string,authorization:string|null}[]=[];
 const transport:typeof fetch=async(input,init)=>{
  const path=String(input), authorization=new Headers(init?.headers).get('Authorization');
  seen.push({path,authorization});
  let value:unknown;
  if(path===PREFIX+'auth/challenge'){
   const now=Math.floor(Date.now()/1000);
   value={wire_base64:base64.encode(encode('WalletChallengeV1',{protocol_version:'1',chain_id:ctx.chain_id,genesis_hash:ctx.genesis_hash,owner:key.owner,server_origin:'http://127.0.0.1:5173',audience:'exchange-api',challenge_nonce:'aa'.repeat(32),issued_at:String(now),expiry_time:String(now+100)}))};
  }else if(path===PREFIX+'auth/session')value={token:'synthetic-private-token'};
  else if(path===PREFIX+'chain/account')return new Response('{}',{status:401});
  else {
   assert.equal(authorization,'Bearer synthetic-private-token');
   value=path===PREFIX+'capabilities'?{...fixture.receipt,api_prefix:PREFIX,signed_result_query:true,automatic_withdraw:false,ws:false}:{...structuredClone(fixture.other_projection),owner:key.owner,received_at_unix_ms:String(Date.now()),withdraw_frozen:true,withdraw_ready:true};
  }
  return new Response(JSON.stringify(value));
 };
 let accountArgs:unknown[]=[];
 const port:ChainPort={account:async(...args)=>{accountArgs=args;const r=await transport(PREFIX+'chain/account',{method:'GET'});if(!r.ok)throw Error('HTTP_401');return r.json();},broadcast:async()=>{throw Error('unexpected broadcast');},result:async()=>{throw Error('unexpected result');}};
 const client=new LocalClient(ctx,transport,port,true,true);
 try{
  client.select(key);await client.login('http://127.0.0.1:5173');assert.equal(client.canWithdraw(),true);
  await assert.rejects(()=>client.withdraw('DEVBASE','1'),/HTTP_401/);
  assert.deepEqual(accountArgs,[key.address]);
  assert.equal(seen.find(r=>r.path===PREFIX+'chain/account')?.authorization,null);
  assert.equal(client.history.length,0);
  client.select();assert.equal(client.canWithdraw(),false);
 }finally{key.destroy();client.destroy();}
});
