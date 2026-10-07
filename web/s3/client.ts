import { Projection, capability, context, envelope, PREFIX, type Context } from './state.ts';
import { LocalKey } from './key.ts';
import { base64, integer, hex, type Input } from './direct-codec.ts';
import { sha256 } from '../src/wallet.ts';
export interface DirectAccount {
  context: Context; owner: string; public_key_base64: string; account_number: string; sequence: string; owner_epoch: string;
  observed_height: string; received_at_unix_ms: string; gas_atoms: string;
}
// Trusted local Chain adapter boundary. No server signer.
// Injection remains available for component tests; authenticated() uses fixed L-R HTTP routes.
export interface ChainPort {
  account(address: string): Promise<DirectAccount>;
  broadcast(tx_bytes: string): Promise<unknown>;
  result(tx_hash: string): Promise<{context: Context; tx_bytes: string; tx_hash: string; height: string; code: string; state: string}>;
}
export interface Entry {owner: string; input: Input; tx_bytes: string; tx_hash: string; state: 'SUBMISSION_UNKNOWN'|'COMMITTED'|'REJECTED_FINAL'; height?: string}
const id=()=>hex(crypto.getRandomValues(new Uint8Array(32)));
export class LocalClient {
  readonly projection: Projection; readonly history: Entry[]=[];
  #token=''; #key?: LocalKey; #busy=new Set<string>(); #capable=false;
  #chain?: ChainPort; #requests=new Set<AbortController>();
  receipt='';
  readonly ctx: Context; readonly transport: typeof fetch; readonly enabled: boolean; readonly acknowledge: boolean;
  constructor(ctx: Context, transport: typeof fetch, chain?: ChainPort, enabled=false, acknowledge=false) {
    this.ctx=ctx;this.transport=transport;this.#chain=chain;this.enabled=enabled;this.acknowledge=acknowledge;
    this.projection=new Projection(ctx);
  }
  // Fixed same-origin routes; the adapter and bearer never leave this client.
  static authenticated(ctx: Context, transport: typeof fetch, enabled=false, acknowledge=false) {
    const client=new LocalClient(ctx,transport,undefined,enabled,acknowledge);
    client.#chain={
      account:async(address)=>{
        if(address!==client.#key?.address)throw Error('ACCOUNT_CHANGED');
        return client.#chainRequest('chain/account');
      },
      broadcast:async(tx_bytes)=>client.#chainRequest('chain/broadcast',{tx_bytes}),
      result:async(tx_hash)=>client.#chainRequest('chain/result',{tx_hash}),
    };
    return client;
  }
  async #chainRequest(path: 'chain/account'|'chain/broadcast'|'chain/result', body?: unknown) {
    if(!this.#key||!this.#token||!this.#capable)throw Error('SESSION_REQUIRED');
    const g=this.projection.generation;
    try{return await this.#request(path,body);}
    catch(e){if(g===this.projection.generation)this.projection.close('CHAIN_REQUEST_FAILED');throw e;}
  }
  revokeSession(){this.select(this.#key);}
  select(key?: LocalKey) {for(const pending of this.#requests)pending.abort();this.#requests.clear();this.#key=key;this.#token='';this.#capable=false;this.receipt='';this.projection.select(key?.owner??'');}
  destroy(){this.#key?.destroy();this.select();}
  async #request(path: string, body?: unknown, token=this.#token) {
    if(!this.enabled||!this.acknowledge)throw Error('TWO_OPT_INS_REQUIRED');
    const generation=this.projection.generation,controller=new AbortController(),started=performance.now();
    this.#requests.add(controller);
    const signal=AbortSignal.any([controller.signal,AbortSignal.timeout(2000)]);
    const current=()=>{
      if(generation!==this.projection.generation)throw Error('ACCOUNT_CHANGED');
      if(signal.aborted||performance.now()-started>2000)throw Error('REQUEST_EXPIRED');
    };
    try {
      const r=await this.transport(PREFIX+path,{method:body===undefined?'GET':'POST',headers:{'Content-Type':'application/json',...(token?{Authorization:`Bearer ${token}`}:{})},...(body===undefined?{}:{body:JSON.stringify(body)}),cache:'no-store',redirect:'error',signal});
      current();
      if(!r.ok){
        if(r.status===401||r.status===403)this.revokeSession();
        throw Error(r.status===503?'RECOVERY_REQUIRED':`HTTP_${r.status}`);
      }
      const value=await r.json();current();return value;
    }finally{this.#requests.delete(controller);}
  }
  async login(origin: string) {
    const key=this.#key;if(!key)throw Error('KEY_REQUIRED');
    this.revokeSession();
    const g=this.projection.generation;
    if(!['http://127.0.0.1:5173','http://localhost:5173'].includes(origin))throw Error('ORIGIN_REJECTED');
    try {
      const challenge=await this.#request('auth/challenge',{owner:key.owner,origin,audience:'exchange-api'});
      if(g!==this.projection.generation)return;
      const session=await this.#request('auth/session',key.challenge(challenge.wire_base64,this.ctx,origin));
      if(g!==this.projection.generation)return;
      if(typeof session.token!=='string'||!session.token)throw Error('AUTH_RESPONSE');
      this.#token=session.token;
      const cap=await this.#request('capabilities');if(g!==this.projection.generation)return;
      capability(cap,this.ctx);this.#capable=true;await this.refresh();
    }catch(e){if(g===this.projection.generation){this.#capable=false;this.projection.close('AUTH_OR_CAPABILITY_FAILED');}throw e;}
  }
  async refresh() {
    const g=this.projection.generation,start=performance.now(),observation=this.projection.beginObservation();
    if(!this.#capable)throw Error('CAPABILITY_REQUIRED');
    try {const v=await this.#request('account');return this.projection.accept(v,g,Date.now(),performance.now()-start,observation);}
    catch(e){if(g===this.projection.generation)this.projection.close((e as Error).message);return false;}
  }
  async prepare(abort=false) {
    const key=this.#key,g=this.projection.generation;
    if(!key||!this.#capable||!this.projection.open()||this.#busy.has(key.owner)||this.history.some(e=>e.owner===key.owner&&e.state==='SUBMISSION_UNKNOWN'))throw Error('WITHDRAW_HELD');
    this.#busy.add(key.owner);
    try {
      const r=await this.#request(abort?'withdraw/abort':'withdraw/prepare',{context:this.ctx,request_id:id()});
      if(g!==this.projection.generation)return;
      envelope(r,this.ctx);integer(r.command_result.command_seq);this.receipt=`개발 명령 결과 ${r.command_result.code} / ${r.command_result.state} — 체인 확정 아님`;await this.refresh();
    }catch(e){if(g===this.projection.generation)this.projection.close('PREPARE_RESULT_UNKNOWN');throw e;}
    finally{this.#busy.delete(key.owner);}
  }
  canWithdraw() {const k=this.#key;return !!k&&!!this.#chain&&this.#capable&&this.projection.ready()&&this.history.filter(e=>e.owner===k.owner&&e.height).every(e=>integer(this.projection.view!.observed_height)>=integer(e.height!))&&!this.#busy.has(k.owner)&&!this.history.some(e=>e.owner===k.owner&&e.state==='SUBMISSION_UNKNOWN');}
  async withdraw(denom: Input['denom'], amount: string) {
    const key=this.#key,g=this.projection.generation;
    if(!key||!this.canWithdraw())throw Error('WITHDRAW_HELD');
    this.#busy.add(key.owner);
    try {
      const started=performance.now();
      const a=await this.#chain!.account(key.address);
      if(performance.now()-started>2000)throw Error('DIRECT_ACCOUNT_STALE');
      if(g!==this.projection.generation||!this.projection.ready())throw Error('ACCOUNT_CHANGED_OR_STALE');
      context(a.context,this.ctx);
      if(a.owner!==key.address||a.public_key_base64!==base64.encode(key.publicKey))throw Error('ACCOUNT_KEY_MISMATCH');
      for(const field of ['account_number','sequence','owner_epoch','observed_height','received_at_unix_ms','gas_atoms'] as const)integer(a[field]);
      const age=BigInt(Date.now())-integer(a.received_at_unix_ms);
      if(age<0n||age>2000n||a.observed_height!==this.projection.view!.observed_height||integer(a.gas_atoms)<1000n)throw Error('DIRECT_ACCOUNT_STALE');
      const row=this.projection.view!.ledger.find(r=>r.denom===denom);
      if(!row||integer(amount,1000000000000n)<1n||integer(amount)>integer(row.C,(1n<<128n)-1n))throw Error('INSUFFICIENT_BALANCE');
      const input: Input={operation:'WITHDRAW',denom,amount_atoms:amount,owner:key.address,request_id:id(),expected_epoch:a.owner_epoch,expiry_height:(integer(a.observed_height)+100n).toString(),genesis_hash:this.ctx.genesis_hash,chain_id:this.ctx.chain_id,account_number:a.account_number,sequence:a.sequence,fee_atoms:'1000',gas_limit:'500000'};
      const entry: Entry={owner:key.owner,input,...key.direct(input),state:'SUBMISSION_UNKNOWN'};
      this.history.push(entry); // Latch UNKNOWN before any network effect.
      this.projection.close('SUBMISSION_UNKNOWN');
      try {await this.#chain!.broadcast(entry.tx_bytes);}catch{/* Query only. Never generate another TX. */}
      return entry;
    }finally{this.#busy.delete(key.owner);}
  }
  async resolve(entry: Entry) {
    const g=this.projection.generation,owner=this.#key?.owner;
    if(!owner||entry.owner!==owner||!this.history.includes(entry)||entry.state!=='SUBMISSION_UNKNOWN'||!this.#chain)return;
    try {
      const r=await this.#chain.result(entry.tx_hash);
      if(g!==this.projection.generation||owner!==this.#key?.owner)return;
      context(r.context,this.ctx);
      if(r.tx_hash.toLowerCase()!==entry.tx_hash||r.tx_bytes!==entry.tx_bytes||hex(sha256(base64.decode(r.tx_bytes)))!==entry.tx_hash||integer(r.height)<=integer(entry.input.expiry_height)-100n||!['COMMITTED','REJECTED_FINAL'].includes(r.state)||(r.state==='COMMITTED')!==(integer(r.code)===0n))return;
      entry.state=r.state as Entry['state'];entry.height=r.height;
      // No local debit or unfreeze. Only a fresh, same-revision account can reopen.
    }catch{/* NOT_FOUND, timeout and invalid evidence retain UNKNOWN. */}
  }
}
