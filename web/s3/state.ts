import { integer } from './direct-codec.ts';
import { canonical } from '../s2/state.ts';
export type Context = Record<string, string>;
export const PREFIX = '/dev-local/v1/';
export const PUBLIC_RECEIPT_VERSION = 's3-dev-local-account/1';
export const PUBLIC_RECEIPT_SCHEMA_SHA256 = '2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b';
export const TRUSTED_RECEIPT_VERSION = 's3-dev-local/1';
export const ASSURANCE = 'LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE · durable_ack=false · UNPROVEN_HOST_SPACE';
export function context(actual: Context, expected: Context) {
  if (canonical(actual) !== canonical(expected) || actual.service_schema !== 's3/3' || actual.chain_id !== 'nus-s3-dev-1') throw Error('CONTEXT_MISMATCH');
}
export function envelope(v: any, ctx: Context) {
  context(v.context, ctx);
  if (v.envelope_version !== 's3-dev-local/1' || v.profile_id !== 's3-dev-local-v1' || v.durable_ack !== false || v.storage_assurance !== 'UNPROVEN_HOST_SPACE' || v.development_receipt !== 'LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE') throw Error('DEVELOPMENT_GUARANTEE');
}
export function capability(v: any, ctx: Context) {
  envelope(v, ctx);
  if (v.api_prefix !== PREFIX || v.signed_result_query !== true || v.automatic_withdraw !== false || v.ws !== false ||
    v.public_receipt_version !== PUBLIC_RECEIPT_VERSION || v.public_receipt_schema_sha256 !== PUBLIC_RECEIPT_SCHEMA_SHA256 ||
    v.trusted_receipt_version !== TRUSTED_RECEIPT_VERSION) throw Error('RECEIPT_SCHEMA');
}
export interface Ledger { denom: string; C: string; R: string; D: string; P: string; A: string }
export interface Account {
  context: Context; owner: string; revision: string; gate: string; durable_ack: false; storage_assurance: string;
  fresh: boolean; indexer_height: string; observed_height: string; snapshot_id: string; received_at_unix_ms: string; query_latency_ms: string;
  ledger: Ledger[]; withdraw_frozen: boolean; withdraw_ready: boolean;
  orders: any[]; fills: any[]; batches: any[];
}
// Service availability permits preparation; direct signing requires every hold cleared.
// Call only after validating the complete account projection.
function withdrawalReady(v: Account) {
  return v.withdraw_ready===true && v.withdraw_frozen===true &&
    v.ledger.every(r=>r.R==='0'&&r.D==='0'&&r.P==='0') &&
    !v.batches.some(b=>!['COMMITTED','CORRECTED','VOID'].includes(b.state)) &&
    !v.fills.some(f=>['PENDING','SUBMISSION_UNKNOWN'].includes(f.state));
}
export interface Observation { readonly generation: number; readonly sequence: number; readonly barrier: number }
export class Projection {
  generation = 0; owner = ''; view?: Account; reason = 'NOT_CONNECTED';
  #sequence = 0; #observedSequence = 0; #barrier = 0;
  #received = 0; #wall = 0; #age = Infinity; #resync = false;
  readonly ctx: Context; readonly clock: () => number;
  constructor(ctx: Context, clock = () => performance.now()) {this.ctx=ctx;this.clock=clock;}
  select(owner: string) { this.generation++; this.owner=owner; this.view=undefined; this.reason='NOT_CONNECTED'; this.#age=Infinity; this.#resync=false; }
  // Every hold prevents already in-flight queries from reopening, regardless of issue order.
  close(reason: string) { this.#barrier++; this.reason=reason; this.#age=Infinity; }
  beginObservation(): Observation {return {generation:this.generation,sequence:++this.#sequence,barrier:this.#barrier};}
  accept(v: Account, generation: number, now: number, elapsed: number, observation=this.beginObservation()): boolean {
    if (generation !== this.generation || observation.generation !== this.generation) return false;
    const superseded=observation.sequence<=this.#observedSequence || observation.barrier!==this.#barrier;
    try {
      context(v.context,this.ctx);
      if(v.owner!==this.owner)throw Error('ACCOUNT_MISMATCH');
      if(v.durable_ack!==false || v.storage_assurance!=='UNPROVEN_HOST_SPACE')throw Error('DEVELOPMENT_GUARANTEE');
      const revision=integer(v.revision),height=integer(v.observed_height);
      if(!Number.isSafeInteger(now)||!Number.isFinite(elapsed)||elapsed<0||elapsed>2000 || now<this.#wall)throw Error('STALE_DELIVERY');
      if(integer(v.indexer_height)!==height || integer(v.query_latency_ms)>2000n || integer(v.received_at_unix_ms)>BigInt(now))throw Error('STALE_OBSERVATION');
      if(!Array.isArray(v.ledger)||v.ledger.length!==2 || new Set(v.ledger.map(r=>r.denom)).size!==2)throw Error('ASSETS');
      for(const row of v.ledger) {
        if(!['DEVBASE','DEVQUOTE'].includes(row.denom))throw Error('ASSETS');
        const [c,r,d,p,a]=[row.C,row.R,row.D,row.P,row.A].map(x=>integer(x,(1n<<128n)-1n));
        if(c-r-d!==a || a<0n || p<0n)throw Error('LEDGER_INVARIANT');
      }
      if(!Array.isArray(v.orders)||!Array.isArray(v.fills)||!Array.isArray(v.batches))throw Error('PROJECTION');
      for(const f of v.fills)if(!['PENDING','SUBMISSION_UNKNOWN','COMMITTED','CORRECTED'].includes(f.state)||integer(f.revision)>revision)throw Error('FILL_STATE');
      for(const b of v.batches) {
        if(!b.batch || !/^[0-9a-f]{64}$/.test(b.batch.batch_id) || !/^[0-9a-f]{64}$/.test(b.batch.batch_hash) || integer(b.revision)>revision)throw Error('BATCH_VIEW');
        integer(b.batch.batch_seq);
        if(b.state==='COMMITTED' && (b.receipt?.disposition!=='COMMITTED'||integer(b.receipt.terminal_height)===0n||integer(b.receipt.terminal_height)>height||!/^[0-9a-f]{64}$/i.test(b.receipt.terminal_tx_hash)||typeof b.receipt.batch_receipt_v2!=='string'||!b.receipt.batch_receipt_v2))throw Error('COMMITTED_RECEIPT_REQUIRED');
      }
      for(const f of v.fills)if(f.state==='COMMITTED'&&!v.batches.some(b=>b.state==='COMMITTED'&&b.batch.batch_id===f.batch?.batch_id))throw Error('COMMITTED_RECEIPT_REQUIRED');
      if(this.view) {
        if(integer(v.received_at_unix_ms)<integer(this.view.received_at_unix_ms))throw Error('OBSERVATION_REGRESSION');
        if(revision<integer(this.view.revision)||height<integer(this.view.observed_height))throw Error('REVISION_REGRESSION');
        const economic=(a: Account)=>canonical([a.revision,a.observed_height,a.snapshot_id,a.ledger,a.orders,a.fills,a.batches,a.withdraw_frozen]);
        if(revision===integer(this.view.revision)&&economic(v)!==economic(this.view))throw Error('REVISION_CONFLICT');
        for(const old of this.view.fills)if(old.state==='COMMITTED'&&!v.fills.some(f=>f.fill_id===old.fill_id&&f.state==='COMMITTED'))throw Error('COMMITTED_REGRESSION');
        if(revision>integer(this.view.revision)+1n&&!this.#resync){this.#resync=true;throw Error('REVISION_GAP_REQUERY');}
      }
      // Validate observation/revision/gap before request-order filtering: even
      // an OPEN response can revoke readiness or expose invalid economic state.
      const serviceOpen=v.fresh===true && v.gate==='OPEN';
      const ready=serviceOpen && withdrawalReady(v);
      if(ready && superseded)return false;
      this.#observedSequence=Math.max(this.#observedSequence,observation.sequence);
      this.view=structuredClone(v);this.#resync=false;this.#received=this.clock();this.#wall=now;
      this.#age=Number(BigInt(now)-integer(v.received_at_unix_ms))+elapsed;
      if(serviceOpen) {
        // A valid withdrawal hold invalidates every query already in flight.
        // Keep preparation available, but never reopen a closed service from
        // a superseded request. A new authoritative query can reopen it.
        if(!superseded || this.reason==='OPEN')this.reason='OPEN';
        if(!ready)this.#barrier++;
      }else this.close('HELD');
      return true;
    }catch(e){this.close((e as Error).message);return false;}
  }
  open(now=Date.now()) {
    const delta=this.clock()-this.#received;
    if(!Number.isFinite(now)||now<this.#wall||delta<0){this.close('CLOCK_REGRESSION');return false;}
    return this.reason==='OPEN' && this.#age+Math.max(delta,now-this.#wall)<=5000;
  }
  ready(now=Date.now()) {
    const v=this.view;
    return !!v && this.open(now) && withdrawalReady(v);
  }
}
