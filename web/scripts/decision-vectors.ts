// Public synthetic vectors only. Private keys never enter output.
import cases from '../../protocol/v1/vectors/decision-port.json' with { type: 'json' };
import signatures from '../../protocol/v1/vectors/signatures.json' with { type: 'json' };
import { hexToBytes as unhex, bytesToHex as hex, encode, frame } from '../src/codec.ts';
import { ml_dsa65, domains } from '../src/wallet.ts';
import { decideOrder, evaluateSnapshot } from '../src/decision.ts';
import { fromFields, context } from '../test/conformance.ts';
const keys = ml_dsa65.keygen(unhex(signatures.test_seed_hex));
const pure = ml_dsa65 as unknown as { sign: (sk: Uint8Array, msg: Uint8Array, ctx: Uint8Array, rnd: Uint8Array) => Uint8Array };
try {
  const template = fromFields('OrderV1', signatures.positives[0].fields);
  const inputs = cases.cap_cases.filter(c => c.expected !== 'INTEGER_RANGE').map(c => ({
    id: c.id, snapshot: { ...cases.decision_cases[0].input.snapshot!, cap: c.cap, active_bps: c.active_bps },
  }));
  inputs.push({ id: 'q-p-1-fee-25', snapshot: { ...cases.decision_cases[0].input.snapshot!, q: '1', p: '1', active_bps: '25', cap: '25' } });
  const generated = inputs.map(({ id, snapshot }) => {
    const order = { ...template, max_qty_lots: snapshot.q, limit_price_ticks: snapshot.p, max_fee_bps: snapshot.cap };
    const body = encode('OrderV1', order), input = frame(domains.OrderV1, body);
    const signature = pure.sign(keys.secretKey, input, new Uint8Array(), unhex(signatures.randomizer_hex));
    return {
      id, canonical_hex: hex(body), sign_bytes_hex: hex(input), public_key_hex: hex(keys.publicKey), signature_hex: hex(signature),
      snapshot, result: decideOrder(body, signature, { ...context('OrderV1', order, keys.publicKey), snapshotId: snapshot.id }, snapshot),
    };
  });
  const synthetic_spec_only = cases.decision_cases.map(c => ({ id: c.id, result: evaluateSnapshot(c.input.authentication_result, c.input.snapshot) }));
  console.log(JSON.stringify({ contract_revision: '549ce150d6a9f21ec30f159d39a4d91c31dbd759', generated, synthetic_spec_only, cross_language: 'NOT_RUN' }, null, 2));
} finally { keys.secretKey.fill(0); }
