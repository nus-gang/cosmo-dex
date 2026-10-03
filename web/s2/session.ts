import { base64 } from '@scure/base';
import { decode, bytesToHex, uint, type Message } from '../src/codec.ts';
import { ml_dsa65, owner, address, sign, validateDevOrder } from '../src/wallet.ts';
import { integer } from '../s1/direct.ts';
import { envelope, join, bytes, hex, type Input } from './direct-codec.ts';
import { sha256 } from '@noble/hashes/sha256';
export { base64 };
export const CONTRACT = '2e103517c344f21c2b97fbe7e977f0e614c32c704b0f54978c5bb1fb3ae6ab0f';
export const CONFIG = '70281595d471947a56d9bf8a97553dd388a85b107c215a95cc6f34ea9f5f321f';
export const ORIGINS = ['http://127.0.0.1:5173', 'http://localhost:5173'];
export interface Context { schema_version: string; chain_id: string; genesis_hash: string; contract_hash: string; config_hash: string; market_id: string; market_config_version: string }
export function context(genesis: string): Context {
  if (!/^[0-9a-f]{64}$/.test(genesis)) throw Error('GENESIS_NOT_PINNED');
  return { schema_version: '1', chain_id: 'nus-s2-dev-1', genesis_hash: genesis, contract_hash: CONTRACT, config_hash: CONFIG, market_id: 'DEVBASE/DEVQUOTE', market_config_version: '1' };
}
export function assertContext(actual: Context, expected: Context) {
  if (!actual || Object.keys(actual).length !== Object.keys(expected).length || Object.keys(expected).some(k => actual[k as keyof Context] !== expected[k as keyof Context])) throw Error('CONTEXT_MISMATCH');
}
export const requestId = () => bytesToHex(crypto.getRandomValues(new Uint8Array(32)));
// A lot and a price tick both have 3 decimal places in the approved profile.
export function units(value: string): string {
  if (!/^(0|[1-9][0-9]*)(\.[0-9]{1,3})?$/.test(value)) throw Error('LOT_TICK: 소수점 이하 최대 3자리');
  const [whole, fraction = ''] = value.split('.');
  const n = BigInt(whole) * 1000n + BigInt(fraction.padEnd(3, '0'));
  if (n < 1n || n > 1000000n) throw Error('MARKET_LIMIT');
  return n.toString();
}
export interface Signed { wire_base64: string; signature_base64: string }
export interface Command extends Signed { context: Context }
// Runtime key is private, never exported, serialized, or accepted from a server.
export class TradingKey {
  #secret: Uint8Array;
  #closed = false;
  readonly publicKey: Uint8Array;
  readonly owner: string;
  readonly address: string;
  constructor() {
    const seed = crypto.getRandomValues(new Uint8Array(32));
    try { const key = ml_dsa65.keygen(seed); this.#secret = key.secretKey; this.publicKey = key.publicKey; }
    finally { seed.fill(0); }
    this.owner = base64.encode(owner(this.publicKey)); this.address = address(this.publicKey);
  }
  direct(input: Input) {
    if (this.#closed) throw Error('SESSION_CLOSED');
    const e = envelope(input, this.publicKey);
    const raw = join(bytes(1, e.body), bytes(2, e.auth), bytes(3, ml_dsa65.sign(this.#secret, e.signDoc)));
    if (raw.length > 16384) throw Error('TX_SIZE');
    return { tx_bytes: base64.encode(raw), tx_hash: hex(sha256(raw)).toUpperCase() };
  }
  destroy() { this.#secret.fill(0); this.#closed = true; }
  #sign(name: 'OrderV1' | 'CancelV1' | 'WalletChallengeV1', message: Message): Signed {
    if (this.#closed) throw Error('SESSION_CLOSED');
    const s = sign(name, message, this.#secret);
    return { wire_base64: base64.encode(s.body), signature_base64: base64.encode(s.signature) };
  }
  challenge(wire: string, ctx: Context, origin: string, now: string): Signed {
    const m = decode('WalletChallengeV1', base64.decode(wire));
    if (!ORIGINS.includes(origin) || m.protocol_version !== '1' || m.chain_id !== ctx.chain_id || m.genesis_hash !== ctx.genesis_hash || m.owner !== this.owner || m.server_origin !== origin || m.audience !== 'exchange-api') throw Error('CHALLENGE_CONTEXT');
    const issued = uint(m.issued_at, 64), expiry = uint(m.expiry_time, 64), time = integer(now);
    if (time < issued || time >= expiry || expiry <= issued || expiry - issued > 120n) throw Error('CHALLENGE_EXPIRED');
    const signed = this.#sign('WalletChallengeV1', m);
    if (signed.wire_base64 !== wire) throw Error('NON_CANONICAL_WIRE');
    return signed;
  }
  order(ctx: Context, epoch: string, height: string, side: 'BUY' | 'SELL', tif: 'GTC' | 'IOC', quantity: string, price: string, id = requestId()): Command {
    if (!['BUY', 'SELL'].includes(side) || !['GTC', 'IOC'].includes(tif)) throw Error('MARKET_LIMIT');
    integer(epoch); const expiry = integer(height) + 100n; integer(expiry.toString());
    const m: Message = { protocol_version: '1', chain_id: ctx.chain_id, genesis_hash: ctx.genesis_hash, exchange_module_id: 'x/exchange', market_id: ctx.market_id, market_config_version: ctx.market_config_version, owner: this.owner, owner_pubkey: base64.encode(this.publicKey), owner_epoch: epoch, order_id: id, side: side === 'BUY' ? '1' : '2', order_type: tif === 'GTC' ? '1' : '2', limit_price_ticks: units(price), max_qty_lots: units(quantity), max_fee_bps: '0', fee_asset_policy_id: 'RECEIVE_ASSET_V1', expiry_height: expiry.toString() };
    validateDevOrder(m, '0'); return { context: ctx, ...this.#sign('OrderV1', m) };
  }
  cancel(ctx: Context, epoch: string, height: string, orderId: string, hash: string, nonce = requestId()): Command {
    integer(epoch); const expiry = integer(height) + 100n; integer(expiry.toString());
    return { context: ctx, ...this.#sign('CancelV1', { protocol_version: '1', chain_id: ctx.chain_id, genesis_hash: ctx.genesis_hash, exchange_module_id: 'x/exchange', market_id: ctx.market_id, owner: this.owner, owner_epoch: epoch, order_id: orderId, order_hash: hash, cancel_nonce: nonce, expiry_height: expiry.toString() }) };
  }
}
