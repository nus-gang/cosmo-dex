import { base64, TradingKey, context, assertContext, requestId, type Command } from './session.ts';
import { Views, canonical, type View, type Book } from './state.ts';
import { sha256 } from '@noble/hashes/sha256';
import { frame, bytesToHex } from '../src/codec.ts';
import { integer } from '../s1/direct.ts';
export interface Submission { kind: 'ORDER' | 'CANCEL'; owner: string; id: string; epoch: string; body: Command; state: string; receipt?: Record<string, unknown> }
export class TradingClient {
  readonly keys: TradingKey[];
  readonly submissions: Submission[] = [];
  readonly views: Views;
  #token = '';
  #expiry = 0n;
  #selected = 0;
  #closed = false;
  #login = 0;
  readonly origin: string;
  readonly transport: typeof fetch;
  constructor(genesis: string, origin: string, transport: typeof fetch = fetch.bind(globalThis), keys = [new TradingKey(), new TradingKey()]) {
    this.origin = origin; this.transport = transport; this.keys = keys;
    this.views = new Views(context(genesis)); this.views.select(this.keys[0].owner);
  }
  get selected() { return this.#selected; }
  get key() { return this.keys[this.#selected]; }
  get authenticated() { return !!this.#token && BigInt(Math.floor(Date.now()/1000)) < this.#expiry; }
  select(index: number) {
    if (this.#closed || !this.keys[index]) throw Error('ACCOUNT');
    const old = this.#token; this.#token = ''; this.#expiry = 0n; this.#login++;
    this.#selected = index; this.views.select(this.key.owner);
    if (old) void this.#request('/s2/auth/logout', undefined, old, 'POST').catch(() => {});
  }
  close() { this.select(this.#selected); this.#closed = true; this.keys.forEach(k => k.destroy()); }
  async #request(path: string, body?: unknown, token = '', method?: string) {
    const r = await this.transport(path, { method: method ?? (body === undefined ? 'GET' : 'POST'), headers: { ...(body === undefined ? {} : { 'Content-Type': 'application/json' }), ...(token ? { Authorization: `Bearer ${token}` } : {}) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }), cache: 'no-store', redirect: 'error', signal: AbortSignal.timeout(5000) });
    const value = await r.json(); return { ok: r.ok, status: r.status, value };
  }
  #current(generation: number) { return !this.#closed && generation === this.views.generation; }
  async login() {
    const g = this.views.generation, key = this.key, attempt = ++this.#login;
    const network = await this.#request('/s2/network');
    if (!this.#current(g) || attempt !== this.#login) return;
    if (!network.ok || network.value.profile !== 's2-local-v1') throw Error('NETWORK_MISMATCH');
    assertContext(network.value.context, this.views.ctx);
    const c = await this.#request('/s2/auth/challenges', { owner: key.owner, origin: this.origin, audience: 'exchange-api' });
    if (!this.#current(g) || attempt !== this.#login) return;
    if (!c.ok) throw Error('CHALLENGE_REJECTED');
    const body = key.challenge(c.value.wire_base64, this.views.ctx, this.origin, Math.floor(Date.now()/1000).toString());
    const s = await this.#request('/s2/auth/sessions', body);
    if (!this.#current(g) || attempt !== this.#login) return;
    const v = s.value, now = BigInt(Math.floor(Date.now()/1000));
    if (!s.ok || v.owner !== key.owner || v.origin !== this.origin || v.audience !== 'exchange-api' || v.genesis_hash !== this.views.ctx.genesis_hash || typeof v.token !== 'string' || base64.decode(v.token).length !== 32 || integer(v.expiry_time) <= now || integer(v.expiry_time) > now+300n) throw Error('SESSION_REJECTED');
    this.#token = v.token; this.#expiry = integer(v.expiry_time);
  }
  async refresh() {
    const g = this.views.generation, token = this.#token;
    try {
      if (!this.authenticated) throw Error('REAUTH_REQUIRED');
      const book = await this.#request('/s2/book');
      if (!this.#current(g)) return;
      if (!book.ok) throw Error('BOOK_UNAVAILABLE');
      this.views.acceptBook(book.value as Book, g);
      let page = await this.#request('/s2/me', undefined, token);
      if (!this.#current(g)) return;
      if (page.status === 401) { this.#token = ''; throw Error('REAUTH_REQUIRED'); }
      if (!page.ok) throw Error('VIEW_UNAVAILABLE');
      const full = page.value as View;
      // Follow only server cursors, at one immutable seq. Publish no partial history.
      const seen = new Set<string>();
      while (page.value.next_cursor !== 'END') {
        const cursor = page.value.next_cursor;
        if (typeof cursor !== 'string' || !/^[A-Za-z0-9_-]{75}$/.test(cursor) || seen.has(cursor) || seen.size >= 128) throw Error('PAGE_LIMIT');
        seen.add(cursor);
        page = await this.#request(`/s2/me?cursor=${cursor}`, undefined, token);
        if (!this.#current(g)) return;
        const next = page.value as View;
        if (!page.ok || next.owner !== full.owner || next.stream_seq !== full.stream_seq || next.revision !== full.revision || next.snapshot_id !== full.snapshot_id || next.observed_height !== full.observed_height || next.owner_epoch !== full.owner_epoch || canonical(next.ledger) !== canonical(full.ledger)) throw Error('SNAPSHOT_CONFLICT');
        assertContext(next.context, this.views.ctx);
        full.orders.push(...next.orders); full.fills.push(...next.fills); full.status = next.status;
      }
      full.next_cursor = 'END';
      this.views.accept(full, g, Date.now());
    } catch (e) { this.views.disconnect(e instanceof Error ? e.message : 'UNAVAILABLE', g); if (this.#current(g)) throw e; }
  }
  #ready(newOrder = true) {
    if (this.#closed || !this.authenticated || (newOrder && !this.views.open(Date.now())) || !this.views.view) throw Error('ADMISSION_CLOSED');
    if (this.submissions.some(s => s.owner === this.key.owner && s.state === 'SUBMISSION_UNKNOWN')) throw Error('UNKNOWN_RECEIPT_REQUIRED');
    return this.views.view;
  }
  async order(side: 'BUY' | 'SELL', tif: 'GTC' | 'IOC', quantity: string, price: string) {
    const view = this.#ready(), id = requestId();
    const body = this.key.order(this.views.ctx, view.owner_epoch, view.observed_height, side, tif, quantity, price, id);
    return this.#submit({ kind: 'ORDER', owner: this.key.owner, id, epoch: view.owner_epoch, body, state: 'SUBMISSION_UNKNOWN' });
  }
  async cancel(orderId: string) {
    const view = this.#ready(false), order = view.orders.find(o => o.order_id === orderId);
    if (!order) throw Error('ORDER_NOT_FOUND');
    const id = requestId(), body = this.key.cancel(this.views.ctx, order.owner_epoch, view.observed_height, order.order_id, order.order_hash, id);
    return this.#submit({ kind: 'CANCEL', owner: this.key.owner, id, epoch: order.owner_epoch, body, state: 'SUBMISSION_UNKNOWN' });
  }
  async #submit(entry: Submission) {
    // Insert before the first await; double click cannot create another ID while uncertain.
    this.submissions.push(entry);
    const g = this.views.generation, token = this.#token;
    try { const r = await this.#request(entry.kind === 'ORDER' ? '/s2/orders' : '/s2/cancels', entry.body, token); if (this.#current(g)) this.#receipt(entry, r.value); }
    catch { /* UNKNOWN: no new ID, no automatic resend. */ }
    return entry;
  }
  #receipt(entry: Submission, value: Record<string, unknown>) {
    if (value.state !== 'LOCAL_ACCEPTED' && value.state !== 'REJECTED') return;
    // Only a context-bound durable receipt finalizes an uncertain local command.
    if (!value.context || value.request_id !== entry.id || value.owner !== entry.owner || value.kind !== entry.kind) return;
    assertContext(value.context as ReturnType<typeof context>, this.views.ctx);
    if (value.owner_epoch !== entry.epoch || value.durability !== 'LOCAL_FSYNC' || value.replicated !== false) throw Error('RECEIPT_MISMATCH');
    for (const k of ['request_hash', 'snapshot_id', 'result_hash', 'journal_commit_hash']) if (typeof value[k] !== 'string' || !/^[0-9a-f]{64}$/.test(value[k] as string)) throw Error('RECEIPT_MISMATCH');
    integer(value.command_seq as string); integer(value.observed_height as string);
    const hash = bytesToHex(sha256(frame(entry.kind === 'ORDER' ? 'NUS/ORDER/V1' : 'NUS/CANCEL/V1', base64.decode(entry.body.wire_base64))));
    if (value.request_hash !== hash) throw Error('RECEIPT_MISMATCH');
    entry.receipt = value; entry.state = value.state as string;
  }
  async resolve(entry: Submission) {
    if (!this.authenticated || entry.owner !== this.key.owner) throw Error('ACCOUNT_MISMATCH');
    const g = this.views.generation;
    const r = await this.#request(`/s2/me/commands/${entry.kind}/${entry.id}?epoch=${entry.epoch}`, undefined, this.#token);
    if (this.#current(g) && r.ok) this.#receipt(entry, r.value.receipt ?? r.value);
    // NOT_FOUND_AT_SEQ does not prove a lost submission had no effect.
  }
  async retry(entry: Submission) {
    await this.resolve(entry);
    if (entry.state !== 'SUBMISSION_UNKNOWN') return;
    if (!this.authenticated || entry.owner !== this.key.owner) throw Error('ACCOUNT_MISMATCH');
    const g = this.views.generation;
    try { const r = await this.#request(entry.kind === 'ORDER' ? '/s2/orders' : '/s2/cancels', entry.body, this.#token); if (this.#current(g)) this.#receipt(entry, r.value); } catch { /* keep same bytes */ }
  }
}
