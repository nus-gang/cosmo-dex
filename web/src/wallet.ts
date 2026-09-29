import { ml_dsa65 } from '@noble/post-quantum/ml-dsa';
import { sha256 } from '@noble/hashes/sha256';
import { base64, bech32 } from '@scure/base';
import { encode, decode, frame, fail, uint, bytesToHex, type Message } from './codec.ts';
export { ml_dsa65, sha256 };
export const domains = { OrderV1: 'NUS/ORDER/V1', CancelV1: 'NUS/CANCEL/V1', WalletChallengeV1: 'NUS/WALLET_AUTH/V1' } as const;
export type Signable = keyof typeof domains;
export function owner(pk: Uint8Array): Uint8Array { if (pk.length !== 1952) fail('KEY_LENGTH'); return sha256(pk).slice(0, 20); }
export function address(pk: Uint8Array): string { return bech32.encode('nus', bech32.toWords(owner(pk))); }
export function parseAddress(value: string): Uint8Array {
  try { const d = bech32.decode(value as `${string}1${string}`); const raw = Uint8Array.from(bech32.fromWords(d.words));
    if (d.prefix !== 'nus' || value !== bech32.encode('nus', d.words) || raw.length !== 20) fail('ADDRESS_MISMATCH'); return raw;
  } catch { return fail('ADDRESS_MISMATCH'); }
}
export function sign(name: Signable, message: Message, secretKey: Uint8Array): { body: Uint8Array; signature: Uint8Array } {
  if (!Object.hasOwn(domains, name)) fail('CONTEXT_MISMATCH');
  const body = encode(name, message); return { body, signature: ml_dsa65.sign(secretKey, frame(domains[name], body)) };
}
export interface VerificationContext {
  // Provided by a trusted finalized account/config snapshot, never by the submitted request.
  expected: Record<string, string>;
  registeredKey?: { type?: string; bytes: Uint8Array };
  height?: string | null;
  snapshotId?: string | null;
  epoch?: string | null;
  now?: string;
  originAllowlist?: readonly string[];
  audiences?: readonly string[];
}
export function authenticate(name: Signable, body: Uint8Array, signature: Uint8Array, ctx: VerificationContext): Message {
  const m = decode(name, body);
  if (m.protocol_version !== '1') fail('UNSUPPORTED_VERSION');
  const required = name === 'WalletChallengeV1' ? ['chain_id', 'genesis_hash', 'server_origin', 'audience'] : name === 'OrderV1' ? ['chain_id', 'genesis_hash', 'exchange_module_id', 'market_id', 'market_config_version', 'fee_asset_policy_id'] : ['chain_id', 'genesis_hash', 'exchange_module_id', 'market_id'];
  if (required.some(k => typeof ctx.expected[k] !== 'string' || ctx.expected[k] !== m[k])) fail('CONTEXT_MISMATCH');
  if (name === 'WalletChallengeV1') {
    const origin = m.server_origin as string; let canonical = false;
    try { const u = new URL(origin); canonical = u.protocol === 'https:' && u.origin === origin && !u.username && !u.password; } catch { /* rejected below */ }
    if (!canonical || !ctx.originAllowlist?.includes(origin) || !ctx.audiences?.includes(m.audience as string)) fail('CONTEXT_MISMATCH');
  }
  const pk = name === 'OrderV1' ? base64.decode(m.owner_pubkey as string) : ctx.registeredKey?.bytes;
  if (signature.length !== 3309 || (pk && pk.length !== 1952)) fail('KEY_LENGTH');
  if (pk && base64.encode(owner(pk)) !== m.owner) fail('ADDRESS_MISMATCH');
  if (!ctx.registeredKey) fail('ACCOUNT_KEY_UNREGISTERED');
  if (ctx.registeredKey.type === undefined) fail('NOT_CONNECTED');
  if (!pk || ctx.registeredKey.type !== 'ML-DSA-65' || bytesToHex(ctx.registeredKey.bytes) !== bytesToHex(pk)) fail('ACCOUNT_KEY_MISMATCH');
  if (!ml_dsa65.verify(pk, frame(domains[name], body), signature)) fail('INVALID_SIGNATURE');
  return m;
}
export function verify(name: Signable, body: Uint8Array, signature: Uint8Array, ctx: VerificationContext): Message {
  const m = authenticate(name, body, signature, ctx);
  if (name !== 'WalletChallengeV1') {
    if (ctx.epoch === undefined || uint(m.owner_epoch, 64) !== uint(ctx.epoch, 64)) fail('EPOCH_MISMATCH');
    if (uint(ctx.height, 64) >= uint(m.expiry_height, 64)) fail('EXPIRED');
  } else {
    const issued = uint(m.issued_at, 64), expiry = uint(m.expiry_time, 64), now = uint(ctx.now, 64);
    if (now < issued || now >= expiry || expiry <= issued || expiry - issued > 120n) fail('EXPIRED');
  }
  return m;
}
// DEV limits are intentionally separate from m0-crypto fixture validation.
export function validateDevOrder(m: Message, feeBps: string): void {
  const q = uint(m.max_qty_lots, 64), p = uint(m.limit_price_ticks, 64), fee = activeBps(feeBps);
  if (!['1', '2'].includes(m.side as string) || !['1', '2'].includes(m.order_type as string) || q < 1n || q > 1000000n || p < 1n || p > 1000000n || q * p > 1000000000000n) fail('MARKET_LIMIT');
  if (fee > uint(m.max_fee_bps, 32)) fail('FEE_CAP');
}
export function activeBps(value: unknown): bigint {
  let n: bigint;
  try { n = uint(value, 32); } catch { return fail('BPS_RANGE'); }
  if (n > 10000n) fail('BPS_RANGE');
  return n;
}
export function checkFeeCap(cap: unknown, rate: unknown): void {
  const c = uint(cap, 32), b = activeBps(rate);
  if (b > c) fail('FEE_CAP');
}
export function feeAtoms(receive: string, bps: string): string {
  const a = uint(receive, 128), b = activeBps(bps); const product = a * b;
  if (product >= 1n << 256n) fail('INTEGER_RANGE');
  const fee = (product + 9999n) / 10000n; if (b > 0n && fee >= a) fail('FEE_GE_RECEIVE');
  return fee.toString();
}
// Test-only in-memory recovery demonstration. No exported backup or secret logging.
export function recoveryProbe(): boolean {
  const seed = crypto.getRandomValues(new Uint8Array(32));
  const a = ml_dsa65.keygen(seed), b = ml_dsa65.keygen(seed);
  try { const msg = new TextEncoder().encode('S0 recovery probe'); return bytesToHex(a.publicKey) === bytesToHex(b.publicKey) && ml_dsa65.verify(a.publicKey, msg, ml_dsa65.sign(b.secretKey, msg)); }
  finally { seed.fill(0); a.secretKey.fill(0); b.secretKey.fill(0); }
}
