// S3 user DIRECT envelope: same approved small-TX wire, isolated chain domain.
import { sha256 } from '@noble/hashes/sha256';
import { base64, bech32 } from '@scure/base';
export { base64 };
const enc = new TextEncoder();
export const hex = (b: Uint8Array) => Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
export function unhex(s: string): Uint8Array { if (!/^[0-9a-f]{64}$/.test(s)) throw Error('HASH_FORMAT'); return Uint8Array.from(s.match(/../g)!, x => parseInt(x, 16)); }
export function integer(s: string, max = (1n << 64n) - 1n): bigint { if (typeof s !== 'string' || !/^(0|[1-9][0-9]*)$/.test(s) || BigInt(s) > max) throw Error('INTEGER_RANGE'); return BigInt(s); }
export function atoms(s: string): string {
  if (!/^(0|[1-9][0-9]*)(\.[0-9]{1,6})?$/.test(s)) throw Error('금액은 소수점 이하 최대 6자리입니다');
  const [a, b = ''] = s.split('.'); const v = BigInt(a) * 1000000n + BigInt(b.padEnd(6, '0'));
  if (v < 1n || v > 1000000000000n) throw Error('AMOUNT_RANGE'); return v.toString();
}
export function display(s: string): string { const n = integer(s, (1n << 128n) - 1n); return `${n / 1000000n}.${(n % 1000000n).toString().padStart(6, '0')}`; }
export function join(...parts: Uint8Array[]): Uint8Array { const out = new Uint8Array(parts.reduce((n, b) => n + b.length, 0)); let at = 0; for (const b of parts) { out.set(b, at); at += b.length; } return out; }
function varint(n: bigint): Uint8Array { const out = []; do { let b = Number(n & 127n); n >>= 7n; if (n) b |= 128; out.push(b); } while (n); return Uint8Array.from(out); }
export function bytes(tag: number, b: Uint8Array): Uint8Array { return join(varint(BigInt(tag * 8 + 2)), varint(BigInt(b.length)), b); }
const str = (tag: number, s: string) => bytes(tag, enc.encode(s));
const num = (tag: number, s: string) => integer(s) === 0n ? new Uint8Array() : join(varint(BigInt(tag * 8)), varint(integer(s)));
const any = (url: string, value: Uint8Array) => join(str(1, url), bytes(2, value));
export const address = (pk: Uint8Array) => bech32.encode('nus', bech32.toWords(sha256(pk).slice(0, 20)));
export interface Input { operation: 'DEPOSIT' | 'WITHDRAW'; denom: 'DEVBASE' | 'DEVQUOTE'; owner: string; amount_atoms: string; request_id: string; expected_epoch: string; expiry_height: string; genesis_hash: string; chain_id: string; account_number: string; sequence: string; fee_atoms: string; gas_limit: string }
export function envelope(i: Input, pk: Uint8Array) {
  if (pk.length !== 1952 || address(pk) !== i.owner || i.chain_id !== 'nus-s3-dev-1' || !['DEVBASE', 'DEVQUOTE'].includes(i.denom) || !['DEPOSIT', 'WITHDRAW'].includes(i.operation)) throw Error('CONTEXT_MISMATCH');
  if (integer(i.amount_atoms, 1000000000000n) < 1n || integer(i.fee_atoms) < 1n || integer(i.gas_limit) < 1n) throw Error('INTEGER_RANGE');
  integer(i.expected_epoch); integer(i.expiry_height);
  const message = join(str(1, i.owner), str(2, i.denom), str(3, i.amount_atoms), bytes(4, unhex(i.request_id)), str(5, i.expected_epoch), str(6, i.expiry_height), bytes(7, unhex(i.genesis_hash)));
  const body = bytes(1, any(`/nus.exchange.v1.Msg${i.operation === 'DEPOSIT' ? 'Deposit' : 'Withdraw'}`, message));
  const signer = join(bytes(1, any('/cosmos.crypto.mldsa65.PubKey', bytes(1, pk))), bytes(2, bytes(1, num(1, '1'))), num(3, i.sequence));
  const fee = join(bytes(1, join(str(1, 'DEVGAS'), str(2, i.fee_atoms))), num(2, i.gas_limit));
  const auth = join(bytes(1, signer), bytes(2, fee));
  const signDoc = join(bytes(1, body), bytes(2, auth), str(3, i.chain_id), num(4, i.account_number));
  return { message, body, auth, signDoc };
}
