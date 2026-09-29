import schemaJson from '../../protocol/v1/schema.json' with { type: 'json' };
import { base64 } from '@scure/base';
import { bytesToHex, hexToBytes, concatBytes } from '@noble/hashes/utils';
export { bytesToHex, hexToBytes, concatBytes };
export type Message = Record<string, unknown>;
type Field = { tag: number; name: string; type: string; repeated: boolean };
export const schema: Record<string, Field[]> = schemaJson;
export class ContractError extends Error { readonly code: string; constructor(code: string) { super(code); this.code = code; } }
export function fail(code: string): never { throw new ContractError(code); }
export function uint(value: unknown, bits: number): bigint {
  if (typeof value !== 'string' || value.length > Math.ceil(bits * Math.LOG10E * Math.LN2) || !/^(0|[1-9][0-9]*)$/.test(value)) fail('INTEGER_RANGE');
  const n = BigInt(value); if (n >= 1n << BigInt(bits)) fail('INTEGER_RANGE'); return n;
}
export function fixed(n: bigint, size: number): Uint8Array {
  if (n < 0n || n >= 1n << BigInt(size * 8)) fail('INTEGER_RANGE');
  const b = new Uint8Array(size); for (let i = size - 1; i >= 0; i--) { b[i] = Number(n & 255n); n >>= 8n; } return b;
}
export function atoms(value: unknown): Uint8Array { return fixed(uint(value, 128), 16); }
export function fromAtoms(b: Uint8Array): string {
  if (b.length !== 16) fail('NON_CANONICAL_WIRE'); return BigInt('0x' + bytesToHex(b)).toString();
}
function vi(n: bigint): Uint8Array { const a: number[] = []; do { const b = Number(n & 127n); n >>= 7n; a.push(b | (n ? 128 : 0)); } while (n); return Uint8Array.from(a); }
const sizes: Record<string, number> = { h: 32, a: 20, pk: 1952, sig: 3309, atoms: 16 };
function limit(name: string): number { return ({ OrderV1: 8192, CancelV1: 1024, WalletChallengeV1: 1024, BatchV1: 1048576 } as Record<string, number>)[name] ?? 1048576; }
function stringBytes(s: unknown, name: string): Uint8Array {
  if (typeof s !== 'string' || s.length < 1 || s.length > 128 || !/^[A-Za-z0-9._:/-]+$/.test(s)) fail('NON_CANONICAL_WIRE');
  // Origin semantics are checked against the trusted context, not normalized here.
  return new TextEncoder().encode(s);
}
function bytesField(f: Field, v: unknown): Uint8Array {
  if (f.type === 's') return stringBytes(v, f.name);
  if (f.type === 'atoms') return atoms(v);
  if (typeof v !== 'string' || v.length !== (f.type === 'h' ? 64 : 4 * Math.ceil(sizes[f.type] / 3))) fail('NON_CANONICAL_WIRE');
  let b: Uint8Array;
  try { b = f.type === 'h' ? (/^[0-9a-f]{64}$/.test(v) ? hexToBytes(v) : fail('NON_CANONICAL_WIRE')) : base64.decode(v); }
  catch { return fail('NON_CANONICAL_WIRE'); }
  if (b.length !== sizes[f.type] || (f.type !== 'h' && base64.encode(b) !== v)) fail('NON_CANONICAL_WIRE');
  return b;
}
export function encode(name: string, value: Message, depth = 0): Uint8Array {
  if (depth >= 4) fail('RESOURCE_LIMIT');
  const fields = schema[name]; if (!fields || !value || typeof value !== 'object' || Array.isArray(value)) fail('NON_CANONICAL_WIRE');
  if (Object.keys(value).some(k => !fields.some(f => f.name === k))) fail('NON_CANONICAL_WIRE');
  const chunks: Uint8Array[] = []; let length = 0;
  for (const f of fields) {
    const v = value[f.name]; const entries = f.repeated ? v : [v];
    if (!Array.isArray(entries)) fail('NON_CANONICAL_WIRE');
    if (f.repeated && entries.length > (f.name === 'fills' ? 1000 : 100)) fail('RESOURCE_LIMIT');
    for (const entry of entries) {
      const integer = f.type === 'u32' || f.type === 'u64';
      const body = integer ? vi(uint(entry, Number(f.type.slice(1)))) : schema[f.type] ? encode(f.type, entry as Message, depth + 1) : bytesField(f, entry);
      const chunk = concatBytes(vi(BigInt(f.tag * 8 + (integer ? 0 : 2))), ...(integer ? [body] : [vi(BigInt(body.length)), body]));
      length += chunk.length; if (length > limit(name)) fail('RESOURCE_LIMIT'); chunks.push(chunk);
    }
  }
  return concatBytes(...chunks);
}
export function decode(name: string, raw: Uint8Array, depth = 0): Message {
  if (depth >= 4 || raw.length > limit(name)) fail('RESOURCE_LIMIT');
  const fields = schema[name]; if (!fields) fail('NON_CANONICAL_WIRE');
  let pos = 0, last = 0; const seen = new Set<number>(); const out: Message = {};
  for (const f of fields) if (f.repeated) out[f.name] = [];
  function readVI(): bigint {
    let n = 0n;
    for (let i = 0; i < 10; i++) {
      if (pos >= raw.length) fail('NON_CANONICAL_WIRE'); const b = raw[pos++]; n |= BigInt(b & 127) << BigInt(i * 7);
      if (b < 128) { if ((i > 0 && b === 0) || n >= 1n << 64n) fail('NON_CANONICAL_WIRE'); return n; }
    } return fail('NON_CANONICAL_WIRE');
  }
  while (pos < raw.length) {
    const key = readVI(); if (key > 0xffffffffn) fail('NON_CANONICAL_WIRE');
    const tag = Number(key >> 3n), wire = Number(key & 7n); const f = fields.find(f => f.tag === tag);
    if (!f || tag < last || (seen.has(tag) && !f.repeated)) fail('NON_CANONICAL_WIRE'); last = tag; seen.add(tag);
    let value: unknown;
    if (f.type === 'u32' || f.type === 'u64') {
      if (wire !== 0) fail('NON_CANONICAL_WIRE'); const n = readVI(); if (n >= 1n << BigInt(f.type.slice(1))) fail('INTEGER_RANGE'); value = n.toString();
    } else {
      if (wire !== 2) fail('NON_CANONICAL_WIRE'); const n = readVI(); if (n > BigInt(raw.length - pos)) fail('NON_CANONICAL_WIRE');
      const b = raw.subarray(pos, pos + Number(n)); pos += Number(n);
      if (schema[f.type]) value = decode(f.type, b, depth + 1);
      else if (f.type === 's') { try { value = new TextDecoder('utf-8', { fatal: true }).decode(b); } catch { fail('NON_CANONICAL_WIRE'); } stringBytes(value, f.name); }
      else {
        if (b.length !== sizes[f.type]) fail('NON_CANONICAL_WIRE');
        value = f.type === 'atoms' ? fromAtoms(b) : f.type === 'h' ? bytesToHex(b) : base64.encode(b);
      }
    }
    if (f.repeated) { const a = out[f.name] as unknown[]; if (a.length >= (f.name === 'fills' ? 1000 : 100)) fail('RESOURCE_LIMIT'); a.push(value); }
    else out[f.name] = value;
  }
  if (fields.some(f => !f.repeated && !seen.has(f.tag))) fail('NON_CANONICAL_WIRE');
  if (bytesToHex(encode(name, out, depth)) !== bytesToHex(raw)) fail('NON_CANONICAL_WIRE'); return out;
}
export function frame(domain: string, body: Uint8Array): Uint8Array {
  const d = stringBytes(domain, 'domain'); return concatBytes(fixed(BigInt(d.length), 4), d, fixed(BigInt(body.length), 8), body);
}
// JSON.parse alone loses duplicate keys. Scan objects first, including escaped aliases.
export function parseJSON(text: string): Message {
  if (text.length > 2 * 1048576) fail('RESOURCE_LIMIT');
  let p = 0;
  const ws = () => { while (/\s/.test(text[p] ?? '') && p < text.length) p++; };
  function str(): string {
    const start = p++; while (p < text.length) { const c = text[p++]; if (c === '\\') p++; else if (c === '"') return JSON.parse(text.slice(start, p)); } return fail('NON_CANONICAL_WIRE');
  }
  function value(depth: number): void {
    if (depth > 8) fail('RESOURCE_LIMIT'); ws();
    if (text[p] === '{') {
      p++; ws(); const keys = new Set<string>(); if (text[p] === '}') { p++; return; }
      while (p < text.length) { ws(); if (text[p] !== '"') fail('NON_CANONICAL_WIRE'); const k = str(); if (keys.has(k)) fail('NON_CANONICAL_WIRE'); keys.add(k); ws(); if (text[p++] !== ':') fail('NON_CANONICAL_WIRE'); value(depth + 1); ws(); const c = text[p++]; if (c === '}') return; if (c !== ',') fail('NON_CANONICAL_WIRE'); }
    } else if (text[p] === '[') { p++; ws(); if (text[p] === ']') { p++; return; } while (p < text.length) { value(depth + 1); ws(); const c = text[p++]; if (c === ']') return; if (c !== ',') fail('NON_CANONICAL_WIRE'); } }
    else if (text[p] === '"') { str(); return; }
    else { const start = p; while (p < text.length && !/[\s,}\]]/.test(text[p])) p++; if (p === start) fail('NON_CANONICAL_WIRE'); return; }
    fail('NON_CANONICAL_WIRE');
  }
  try { value(0); ws(); if (p !== text.length) fail('NON_CANONICAL_WIRE'); return JSON.parse(text); }
  catch (e) { if (e instanceof ContractError) throw e; return fail('NON_CANONICAL_WIRE'); }
}
