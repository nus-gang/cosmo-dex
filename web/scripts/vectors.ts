import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { ml_dsa65 } from '@noble/post-quantum/ml-dsa';
import { fromFields, runConformance } from '../test/conformance.ts';
import { encode, frame, hexToBytes as unhex, bytesToHex as hex } from '../src/codec.ts';
import { domains, type Signable } from '../src/wallet.ts';
const contract_revision = '889fda0c7181a696b4eb2a2649508c6192af8406';
const original = readFileSync(new URL('../../protocol/v1/vectors/signatures.json', import.meta.url));
const input = process.argv[2] ? readFileSync(resolve(process.argv[2])) : original;
const hash = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');
if (hash(input) !== hash(original)) throw new Error('Pinned signatures.json hash mismatch');
runConformance();
const v = JSON.parse(input.toString());
const names: Record<string, Signable> = { order: 'OrderV1', cancel: 'CancelV1', wallet: 'WalletChallengeV1' };
const pure = ml_dsa65 as unknown as { sign: (sk: Uint8Array, msg: Uint8Array, ctx: Uint8Array, rnd: Uint8Array) => Uint8Array; verify: (pk: Uint8Array, msg: Uint8Array, sig: Uint8Array, ctx: Uint8Array) => boolean };
const keys = ml_dsa65.keygen(unhex(v.test_seed_hex));
try {
  const generated: { id: string; canonical_hex: string; sign_bytes_hex: string; public_key_hex: string; signature_hex: string; valid: boolean }[] = v.positives.map((p: { id: string; fields: (number | string)[][]; signature_hex: string; canonical_hex: string }) => {
    const name = names[p.id], body = encode(name, fromFields(name, p.fields)), input = frame(domains[name], body);
    const signature = pure.sign(keys.secretKey, input, new Uint8Array(), unhex(v.randomizer_hex));
    if (hex(signature) !== p.signature_hex || hex(body) !== p.canonical_hex) throw new Error('KAT mismatch');
    return { id: p.id, canonical_hex: hex(body), sign_bytes_hex: hex(input), public_key_hex: hex(keys.publicKey), signature_hex: hex(signature), valid: true };
  });
  // Output contains only public KAT values. The input seed is already published by protocol; no new recovery secret is persisted.
  writeFileSync('evidence/ts-generated.json', JSON.stringify({ contract_revision, vectors_sha256: hash(input), generated }, null, 2) + '\n');
  const results = generated.map(({ id, sign_bytes_hex, valid }) => ({ id, sign_bytes_hex, valid }));
  for (const n of v.negatives) {
    let valid = false;
    try { valid = pure.verify(unhex(n.public_key_hex), unhex(n.message_hex), unhex(n.signature_hex), unhex(n.context_hex)); } catch { /* malformed signature rejects */ }
    if (valid !== n.expected_crypto_valid) throw new Error('Negative vector mismatch');
    results.push({ id: n.id, sign_bytes_hex: n.message_hex, valid });
  }
  process.stdout.write(JSON.stringify({ contract_revision, vectors_sha256: hash(input), results }) + '\n');
} finally { keys.secretKey.fill(0); }
