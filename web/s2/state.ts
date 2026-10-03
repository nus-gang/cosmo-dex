import { sha256 } from '@noble/hashes/sha256';
import { bytesToHex, frame } from '../src/codec.ts';
import { integer } from '../s1/direct.ts';
import { assertContext, type Context } from './session.ts';
export function canonical(value: unknown): string {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  const object = value as Record<string, unknown>;
  return `{${Object.keys(object).sort().map(k => `${JSON.stringify(k)}:${canonical(object[k])}`).join(',')}}`;
}
export interface Status { context: Context; stream_seq: string; revision: string; mode: string; reason: string; observation: { snapshot_id: string; observed_height: string; fresh: boolean; last_success_age_ms: string; block_age_ms: string; query_latency_ms: string }; durability: string; replicated: boolean; settlement_submission_enabled: boolean }
export interface Order { order_id: string; order_hash: string; owner_epoch: string; admission_seq: string; side: string; order_type: string; limit_price_ticks: string; max_qty_lots: string; remaining_qty_lots: string; filled_qty_lots: string; corrected_qty_lots: string; cancelled_qty_lots: string; state: string; revision: string }
export interface Fill { fill_id: string; own_order_id: string; command_seq: string; match_index: string; quantity_lots: string; execution_price_ticks: string; fee_policy_version: string; fee_base_atoms: string; fee_quote_atoms: string; state: string; reason: string; revision: string }
export interface Ledger { denom: string; C: string; R: string; D: string; P: string; A: string }
export interface View { context: Context; owner: string; owner_epoch: string; stream_seq: string; revision: string; snapshot_id: string; observed_height: string; ledger: Ledger[]; orders: Order[]; fills: Fill[]; next_cursor: string; status: Status }
export interface Book { context: Context; stream_seq: string; revision: string; snapshot_id: string; observed_height: string; bids: { price_ticks: string; qty_lots: string; order_count: string }[]; asks: { price_ticks: string; qty_lots: string; order_count: string }[]; content_hash: string }
// Status revisions advance without command seq; never compare a health hash to an economic snapshot hash.
export class Views {
  #generation = 0;
  #received = 0;
  #status?: Status;
  view?: View;
  book?: Book;
  owner = '';
  reason = 'NOT_CONNECTED';
  readonly ctx: Context;
  constructor(ctx: Context) { this.ctx = ctx; }
  get generation() { return this.#generation; }
  select(owner: string) { this.#generation++; this.owner = owner; this.view = undefined; this.book = undefined; this.#status = undefined; this.#received = 0; this.reason = 'NOT_CONNECTED'; }
  disconnect(reason: string, generation: number) { if (generation === this.#generation) { this.reason = reason; this.#received = 0; } }
  #statusValid(s: Status) {
    assertContext(s.context, this.ctx); integer(s.stream_seq); integer(s.revision); integer(s.observation.observed_height);
    if (s.durability !== 'LOCAL_FSYNC' || s.replicated !== false || s.settlement_submission_enabled !== false) throw Error('STATUS_CONTEXT');
  }
  accept(view: View, generation: number, now: number): boolean {
    if (generation !== this.#generation) return false;
    assertContext(view.context, this.ctx);
    if (view.owner !== this.owner) throw Error('ACCOUNT_MISMATCH');
    const seq = integer(view.stream_seq), rev = integer(view.revision); integer(view.owner_epoch); integer(view.observed_height);
    this.#statusValid(view.status);
    if (view.status.stream_seq !== view.stream_seq || view.status.observation.snapshot_id !== view.snapshot_id || view.status.observation.observed_height !== view.observed_height) throw Error('SNAPSHOT_CONFLICT');
    if (view.ledger.length !== 2 || new Set(view.ledger.map(r => r.denom)).size !== 2) throw Error('ASSET_SET');
    for (const row of view.ledger) {
      if (!['DEVBASE', 'DEVQUOTE'].includes(row.denom)) throw Error('ASSET_SET');
      const [c,r,d,p,a] = ['C','R','D','P','A'].map(k => integer(row[k as keyof Ledger], (1n << 128n)-1n));
      if (c-r-d !== a || a < 0n || p < 0n) throw Error('LEDGER_INVARIANT');
    }
    const orderIds = new Set<string>(), fillIds = new Set<string>();
    for (const o of view.orders) {
      if (!/^[0-9a-f]{64}$/.test(o.order_id) || !/^[0-9a-f]{64}$/.test(o.order_hash) || orderIds.has(o.order_id) || !['BUY','SELL'].includes(o.side) || !['LIMIT_GTC','LIMIT_IOC'].includes(o.order_type) || !['OPEN','PARTIALLY_FILLED','FILLED_PENDING','CANCELLED_OFFCHAIN','EXPIRED','STP_CANCELLED','POLICY_REJECTED_REMAINDER','CORRECTED'].includes(o.state)) throw Error('ORDER_VIEW');
      orderIds.add(o.order_id);
      integer(o.owner_epoch); integer(o.admission_seq); integer(o.revision); integer(o.limit_price_ticks);
      const q=integer(o.max_qty_lots), remaining=integer(o.remaining_qty_lots), filled=integer(o.filled_qty_lots), cancelled=integer(o.cancelled_qty_lots), corrected=integer(o.corrected_qty_lots);
      if (remaining+filled+cancelled !== q || corrected>filled) throw Error('ORDER_QUANTITY');
    }
    for (const f of view.fills) {
      if (!/^[0-9a-f]{64}$/.test(f.fill_id) || fillIds.has(f.fill_id) || !['PENDING','CORRECTED'].includes(f.state)) throw Error('FILL_VIEW');
      fillIds.add(f.fill_id); integer(f.command_seq); integer(f.revision); integer(f.match_index); integer(f.quantity_lots); integer(f.execution_price_ticks);
    }
    if (this.view) {
      if (seq < integer(this.view.stream_seq) || rev < integer(this.view.revision) || integer(view.observed_height) < integer(this.view.observed_height)) return false;
      const economic = ({ status: _s, ...v }: View) => canonical(v);
      if (seq === integer(this.view.stream_seq) && economic(view) !== economic(this.view)) throw Error('SNAPSHOT_CONFLICT');
      for (const [previous, next, key] of [[this.view.orders, view.orders, 'order_id'], [this.view.fills, view.fills, 'fill_id']] as const) {
        for (const item of next) {
          const old = previous.find(v => (v as unknown as Record<string,string>)[key] === (item as unknown as Record<string,string>)[key]);
          if (old && (integer(item.revision) < integer(old.revision) || (item.revision === old.revision && canonical(item) !== canonical(old)))) throw Error('ENTITY_REGRESSION');
        }
      }
    }
    if (this.#status && integer(view.status.revision) < integer(this.#status.revision)) return false;
    if (this.#status && view.status.revision === this.#status.revision && canonical(view.status) !== canonical(this.#status)) throw Error('STATUS_CONFLICT');
    this.view = view; this.#status = view.status; this.#received = now; this.reason = view.status.reason; return true;
  }
  acceptBook(book: Book, generation: number): boolean {
    if (generation !== this.#generation) return false;
    assertContext(book.context, this.ctx);
    const seq = integer(book.stream_seq); integer(book.revision); integer(book.observed_height);
    const { content_hash, ...body } = book;
    const hash = bytesToHex(sha256(frame('NUS/S2/BOOK/V1', new TextEncoder().encode(canonical(body)))));
    if (hash !== content_hash) throw Error('BOOK_HASH');
    if (this.book && seq < integer(this.book.stream_seq)) return false;
    if (this.book && seq === integer(this.book.stream_seq) && content_hash !== this.book.content_hash) throw Error('SNAPSHOT_CONFLICT');
    this.book = book; return true;
  }
  open(now: number): boolean {
    const s = this.#status;
    return this.#received > 0 && now >= this.#received && now-this.#received <= 5000 && s?.mode === 'OPEN' && s.observation.fresh === true && integer(s.observation.last_success_age_ms) <= 5000n && integer(s.observation.block_age_ms) <= 5000n;
  }
}
