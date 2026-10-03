import { type TradingKey, type Context, assertContext, base64, requestId } from './session.ts';
import { integer, type Input } from './direct-codec.ts';
export interface Account {
  state: string; signing_ready: boolean; context: Context; owner: string; owner_base64: string;
  public_key_type: string; public_key_base64: string; account_number: string; sequence: string; owner_epoch: string;
  balances: {denom: string; bank_atoms: string; confirmed_atoms: string}[];
  gas_denom: string; gas_atoms: string; observed_height: string; cursor_height: string;
  block_hash: string; block_time_unix_ms: string; freshness_ms: string; query_latency_ms: string; indexer_mode: string;
}
export interface DirectEntry { input: Input; tx_bytes: string; tx_hash: string; state: 'SUBMISSION_UNKNOWN'|'COMMITTED'|'REJECTED_FINAL'; height?: string }
export function validateAccount(a: Account, key: TradingKey, ctx: Context, now: number) {
  assertContext(a.context, ctx);
  if (a.state !== 'COMMITTED' || a.signing_ready !== true || a.owner !== key.address || a.owner_base64 !== key.owner || a.public_key_type !== '/cosmos.crypto.mldsa65.PubKey' || a.public_key_base64 !== base64.encode(key.publicKey)) throw Error('ACCOUNT_KEY_MISMATCH');
  for (const field of ['account_number','sequence','owner_epoch','gas_atoms','observed_height','cursor_height','block_time_unix_ms','freshness_ms','query_latency_ms'] as const) integer(a[field]);
  if (a.gas_denom !== 'DEVGAS' || a.indexer_mode !== 'DIRECT_COMMITTED_QUERY' || a.observed_height !== a.cursor_height || integer(a.observed_height) === 0n || !/^[0-9a-f]{64}$/i.test(a.block_hash)) throw Error('ACCOUNT_CONTEXT');
  if (integer(a.freshness_ms) > 5000n || integer(a.query_latency_ms) > 2000n || BigInt(now)-integer(a.block_time_unix_ms) > 5000n || integer(a.block_time_unix_ms)-BigInt(now) > 1000n) throw Error('STALE_ACCOUNT');
  if (!Array.isArray(a.balances) || a.balances.length !== 2) throw Error('ASSETS');
  for (const [i,denom] of ['DEVBASE','DEVQUOTE'].entries()) {
    const b=a.balances[i]; if (b.denom !== denom) throw Error('ASSETS');
    integer(b.bank_atoms); integer(b.confirmed_atoms);
  }
}
export class DirectClient {
  readonly history: DirectEntry[] = [];
  #busy = new Set<string>();
  #heights = new Map<string,bigint>();
  readonly ctx: Context;
  readonly transport: typeof fetch;
  constructor(ctx: Context, transport: typeof fetch) {this.ctx=ctx;this.transport=transport;}
  async #get(path: string) {
    const r=await this.transport(path,{cache:'no-store',redirect:'error',signal:AbortSignal.timeout(5000)});
    if(!r.ok)throw Error('DIRECT_UNAVAILABLE');return r.json();
  }
  async account(key: TradingKey) {
    const a: Account=await this.#get(`/s2/accounts/${key.address}`);
    validateAccount(a,key,this.ctx,Date.now());
    const minimum=this.history.filter(e=>e.input.owner===key.address && e.height).reduce((h,e)=>integer(e.height!)>h?integer(e.height!):h,this.#heights.get(key.owner)??0n);
    if(integer(a.observed_height)<minimum)throw Error('HEIGHT_REGRESSION');
    this.#heights.set(key.owner,integer(a.observed_height));return a;
  }
  async submit(key: TradingKey, operation: Input['operation'], denom: Input['denom'], amount: string, current: ()=>boolean) {
    if(this.#busy.has(key.owner)||this.history.some(e=>e.input.owner===key.address&&e.state==='SUBMISSION_UNKNOWN'))throw Error('UNKNOWN_TX_REQUIRED');
    this.#busy.add(key.owner);
    try {
      const a=await this.account(key);if(!current())throw Error('ACCOUNT_CHANGED');
      const balance=a.balances.find(b=>b.denom===denom);if(!balance)throw Error('ASSETS');
      if(integer(amount,1000000000000n)<1n||integer(amount)>integer(operation==='DEPOSIT'?balance.bank_atoms:balance.confirmed_atoms))throw Error('INSUFFICIENT_BALANCE');
      if(integer(a.gas_atoms)<1000n)throw Error('INSUFFICIENT_GAS');
      const input: Input={operation,denom,owner:key.address,amount_atoms:amount,request_id:requestId(),expected_epoch:a.owner_epoch,expiry_height:(integer(a.observed_height)+100n).toString(),genesis_hash:this.ctx.genesis_hash,chain_id:this.ctx.chain_id,account_number:a.account_number,sequence:a.sequence,fee_atoms:'1000',gas_limit:'500000'};
      const entry: DirectEntry={input,...key.direct(input),state:'SUBMISSION_UNKNOWN'};this.history.push(entry);
      try{await this.transport('/s1/txs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({tx_bytes:entry.tx_bytes}),redirect:'error',signal:AbortSignal.timeout(5000)});}catch{/* Never re-sign an uncertain TX. */}
      return entry;
    }finally{this.#busy.delete(key.owner);}
  }
  async resolve(e: DirectEntry) {
    try {
      const r=await this.#get(`/s1/txs/${e.tx_hash}`);
      if(r.tx_hash!==e.tx_hash || !['COMMITTED','REJECTED_FINAL'].includes(r.state) || integer(r.height)===0n || (r.state==='COMMITTED')!==(integer(r.code)===0n))return;
      e.state=r.state;e.height=r.height;
    }catch{/* UNKNOWN and 404 do not prove rejection. */}
  }
}
