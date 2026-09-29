import signatures from '../../protocol/v1/vectors/signatures.json' with { type: 'json' };
import amounts from '../../protocol/v1/vectors/amount-codec.json' with { type: 'json' };
import messages from '../../protocol/v1/vectors/message-codec.json' with { type: 'json' };
import wires from '../../protocol/v1/vectors/wire-cases.json' with { type: 'json' };
import { base64 } from '@scure/base';
import { encode, decode, atoms, fromAtoms, frame, schema, bytesToHex as hex, hexToBytes as unhex, uint, parseJSON, type Message } from '../src/codec.ts';
import { ml_dsa65, sha256, domains, verify, sign, owner, address, parseAddress, recoveryProbe, feeAtoms, validateDevOrder, type Signable, type VerificationContext } from '../src/wallet.ts';
const names: Record<string, Signable> = { order: 'OrderV1', cancel: 'CancelV1', wallet: 'WalletChallengeV1' };
export function fromFields(name: string, fields: (string | number)[][]): Message {
  return Object.fromEntries(fields.map(([tag, type, value]) => {
    const f = schema[name].find(f => f.tag === tag)!; const s = String(value);
    return [f.name, type !== 'hex' ? s : f.type === 'h' ? s : base64.encode(unhex(s))];
  }));
}
export function context(name: Signable, m: Message, pk: Uint8Array): VerificationContext {
  const keys = ['chain_id', 'genesis_hash', 'exchange_module_id', 'market_id', 'market_config_version', 'fee_asset_policy_id', 'server_origin', 'audience'];
  return { expected: Object.fromEntries(keys.filter(k => k in m).map(k => [k, m[k] as string])), registeredKey: { type: 'ML-DSA-65', bytes: pk }, height: '999', epoch: m.owner_epoch as string, now: (BigInt(m.expiry_time as string ?? '1') - 1n).toString(), originAllowlist: [m.server_origin as string], audiences: [m.audience as string] };
}
export function runConformance(): { passed: number; checks: string[] } {
  const checks: string[] = [];
  const ok = (id: string, condition: boolean) => { if (!condition) throw new Error('FAIL ' + id); checks.push(id); };
  const rejects = (id: string, fn: () => unknown, code?: string) => { let error: unknown; try { fn(); } catch (e) { error = e; } ok(id, error instanceof Error && (!code || error.message === code)); };
  const pure = ml_dsa65 as unknown as { sign: (sk: Uint8Array, msg: Uint8Array, ctx: Uint8Array, rnd: Uint8Array) => Uint8Array; verify: (pk: Uint8Array, msg: Uint8Array, sig: Uint8Array, ctx: Uint8Array) => boolean };
  const keys = ml_dsa65.keygen(unhex(signatures.test_seed_hex)); // already-public synthetic KAT input
  try {
    for (const v of signatures.positives) {
      const name = names[v.id], m = fromFields(name, v.fields), body = encode(name, m), pk = unhex(v.public_key_hex), sig = unhex(v.signature_hex);
      const input = frame(domains[name], body);
      ok(v.id + ':canonical', hex(body) === v.canonical_hex);
      ok(v.id + ':roundtrip', JSON.stringify(decode(name, body)) === JSON.stringify(m));
      ok(v.id + ':frame/hash', hex(input) === v.sign_input_hex && hex(sha256(input)) === v.sha256);
      ok(v.id + ':public-key/owner', hex(keys.publicKey) === v.public_key_hex && hex(owner(pk)) === v.owner_raw_hex);
      ok(v.id + ':deterministic-sign', hex(pure.sign(keys.secretKey, input, new Uint8Array(), unhex(signatures.randomizer_hex))) === v.signature_hex);
      ok(v.id + ':verify', ml_dsa65.verify(pk, input, sig));
      const ctx = context(name, m, pk);
      ok(v.id + ':contract-verify', !!verify(name, body, sig, ctx));
      rejects(v.id + ':unregistered', () => verify(name, body, sig, { ...ctx, registeredKey: undefined }), 'ACCOUNT_KEY_UNREGISTERED');
      rejects(v.id + ':wrong-chain', () => verify(name, body, sig, { ...ctx, expected: { ...ctx.expected, chain_id: 'other' } }), 'CONTEXT_MISMATCH');
      const expiry = name === 'WalletChallengeV1' ? { now: m.expiry_time as string } : { height: m.expiry_height as string };
      rejects(v.id + ':expiry-equality', () => verify(name, body, sig, { ...ctx, ...expiry }), 'EXPIRED');
      const corrupt = sig.slice(); corrupt[0] ^= 1;
      rejects(v.id + ':invalid-signature-before-expiry', () => verify(name, body, corrupt, { ...ctx, ...expiry }), 'INVALID_SIGNATURE');
      rejects(v.id + ':short-signature', () => verify(name, body, sig.slice(1), ctx), 'KEY_LENGTH');
      ok(v.id + ':bech32', hex(parseAddress(address(pk))) === v.owner_raw_hex);
      rejects(v.id + ':uppercase-address', () => parseAddress(address(pk).toUpperCase()), 'ADDRESS_MISMATCH');
    }
    for (const v of signatures.negatives) {
      const pk = unhex(v.public_key_hex), input = unhex(v.message_hex), sig = unhex(v.signature_hex), contextBytes = unhex(v.context_hex);
      let result = false; try { result = pure.verify(pk, input, sig, contextBytes); } catch { /* library rejects malformed signature length */ }
      ok(v.id, result === v.expected_crypto_valid);
    }
    for (const [i, v] of amounts.cases.entries()) {
      if (v.expected === 'OK') ok('amount:' + i, hex(atoms(v.api_json)) === v.wire_hex && fromAtoms(unhex(v.wire_hex!)) === v.api_json);
      else rejects('amount:' + i, () => 'api_json' in v ? atoms(v.api_json) : fromAtoms(unhex(v.wire_hex!)), v.expected);
    }
    for (const v of messages.positives) {
      const b = encode(v.message, v.api_json as Message);
      ok(v.id + ':message-bytes', hex(b) === v.canonical_hex);
      ok(v.id + ':message-json', JSON.stringify(decode(v.message, b)) === JSON.stringify(v.api_json));
      if ('payment_frame_hex' in v) ok(v.id + ':payment', hex(frame('NUS/PAYMENT_ID/V1', b)) === v.payment_frame_hex && hex(sha256(frame('NUS/PAYMENT_ID/V1', b))) === v.payment_hash);
    }
    for (const v of [...wires.cases, ...messages.wire_cases]) {
      if (v.expected === 'CANONICAL') ok(v.id, !!decode(v.message, unhex(v.wire_hex)));
      else rejects(v.id, () => decode(v.message, unhex(v.wire_hex)), v.expected);
    }
    for (const bits of [32, 64, 128]) {
      const max = (1n << BigInt(bits)) - 1n; ok('uint-max-' + bits, uint(max.toString(), bits) === max);
      for (const v of [(max + 1n).toString(), '-1', '+1', '01', '1e0', '1.0', ' 1', 1, null]) rejects('uint-invalid-' + bits + ':' + v, () => uint(v, bits), 'INTEGER_RANGE');
    }
    for (const text of ['{"a":"1","a":"2"}', '{"a":"1","\\u0061":"2"}', '{"x":{"a":1,"a":2}}']) rejects('duplicate-json:' + text, () => parseJSON(text), 'NON_CANONICAL_WIRE');
    ok('json-string-braces', parseJSON('{"x":"a}\\\"b"}').x === 'a}"b');
    const v = signatures.positives[0], m = fromFields('OrderV1', v.fields), ctx = context('OrderV1', m, keys.publicKey);
    const randomSeed = crypto.getRandomValues(new Uint8Array(32)), fresh = ml_dsa65.keygen(randomSeed); randomSeed.fill(0);
    try {
      const freshOrder = { ...m, owner: base64.encode(owner(fresh.publicKey)), owner_pubkey: base64.encode(fresh.publicKey) };
      const signed = sign('OrderV1', freshOrder, fresh.secretKey);
      ok('fresh-key-sign-verify', !!verify('OrderV1', signed.body, signed.signature, { ...ctx, registeredKey: { type: 'ML-DSA-65', bytes: fresh.publicKey } }));
      rejects('wrong-registered-key', () => verify('OrderV1', unhex(v.canonical_hex), unhex(v.signature_hex), { ...ctx, registeredKey: { type: 'ML-DSA-65', bytes: fresh.publicKey } }), 'ACCOUNT_KEY_MISMATCH');
      const wrong = sign('OrderV1', { ...freshOrder, owner: m.owner }, fresh.secretKey);
      rejects('owner-key-binding', () => verify('OrderV1', wrong.body, wrong.signature, ctx), 'ADDRESS_MISMATCH');
      rejects('epoch', () => verify('OrderV1', unhex(v.canonical_hex), unhex(v.signature_hex), { ...ctx, epoch: '999' }), 'EPOCH_MISMATCH');
    } finally { fresh.secretKey.fill(0); }
    const w = signatures.positives[2], wm = fromFields('WalletChallengeV1', w.fields), wc = context('WalletChallengeV1', wm, keys.publicKey);
    for (const [id, update, change, error] of [
      ['at-issued', {}, { now: wm.issued_at }, null],
      ['before-issued', {}, { now: (BigInt(wm.issued_at as string) - 1n).toString() }, 'EXPIRED'],
      ['ttl121', { issued_at: (BigInt(wm.expiry_time as string) - 121n).toString() }, {}, 'EXPIRED'],
      ['zero-ttl', { issued_at: wm.expiry_time }, {}, 'EXPIRED'],
      ['origin-path', { server_origin: 'https://wallet.invalid/path' }, {}, 'CONTEXT_MISMATCH'],
      ['origin-uppercase', { server_origin: 'https://WALLET.invalid' }, {}, 'CONTEXT_MISMATCH'],
      ['origin-port443', { server_origin: 'https://wallet.invalid:443' }, {}, 'CONTEXT_MISMATCH'],
      ['audience', { audience: 'other' }, {}, 'CONTEXT_MISMATCH'],
    ] as [string, Message, Partial<VerificationContext>, string | null][]) {
      const signed = sign('WalletChallengeV1', { ...wm, ...update }, keys.secretKey);
      const call = () => verify('WalletChallengeV1', signed.body, signed.signature, { ...wc, ...change });
      if (error) rejects('wallet-policy:' + id, call, error); else ok('wallet-policy:' + id, !!call());
    }
    const maxOrder = { ...m, owner_epoch: '18446744073709551615', expiry_height: '18446744073709551615', max_qty_lots: '18446744073709551615' };
    const maxSigned = sign('OrderV1', maxOrder, keys.secretKey);
    ok('u64-sign-roundtrip', !!verify('OrderV1', maxSigned.body, maxSigned.signature, { ...ctx, epoch: maxOrder.owner_epoch, height: '18446744073709551614' }));
    rejects('u64-max-expiry-equality', () => verify('OrderV1', maxSigned.body, maxSigned.signature, { ...ctx, epoch: maxOrder.owner_epoch, height: maxOrder.expiry_height }), 'EXPIRED');
    rejects('dev-rejects-codec-valid-max', () => validateDevOrder(maxOrder, '0'), 'MARKET_LIMIT');
    rejects('wire-resource', () => decode('OrderV1', new Uint8Array(8193)), 'RESOURCE_LIMIT');
    rejects('unknown-api-field', () => encode('OrderV1', { ...m, extra: '1' }), 'NON_CANONICAL_WIRE');
    rejects('noncanonical-base64', () => encode('OrderV1', { ...m, owner: (m.owner as string).replace(/=/g, '') }), 'NON_CANONICAL_WIRE');
    ok('recover-in-memory', recoveryProbe());
    ok('fee-zero', feeAtoms('1', '0') === '0'); ok('fee-ceil', feeAtoms('1001', '25') === '3');
    rejects('fee-ge-receive', () => feeAtoms('1', '25'), 'FEE_GE_RECEIVE');
    rejects('dev-qty-limit', () => validateDevOrder({ ...m, max_qty_lots: '1000001' }, '0'), 'MARKET_LIMIT');
    return { passed: checks.length, checks };
  } finally { keys.secretKey.fill(0); }
}
