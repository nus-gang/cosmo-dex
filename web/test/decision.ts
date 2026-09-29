import vectors from '../../protocol/v1/vectors/decision-port.json' with { type: 'json' };
import signatures from '../../protocol/v1/vectors/signatures.json' with { type: 'json' };
import { hexToBytes, type Message } from '../src/codec.ts';
import { feeAtoms, checkFeeCap, sign, type VerificationContext } from '../src/wallet.ts';
import { apiError, decideOrder, evaluateSnapshot, type Snapshot } from '../src/decision.ts';

export function decisionChecks(m: Message, ctx: VerificationContext, secretKey: Uint8Array): string[] {
  ctx = { ...ctx, snapshotId: 'synthetic-1' };
  const checks: string[] = [];
  const eq = (id: string, actual: unknown, expected: unknown) => {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(id + ': ' + JSON.stringify({ actual, expected }));
    checks.push('rc3:' + id);
  };
  const code = (fn: () => unknown) => { try { return fn() ?? 'OK'; } catch (e) { if (e instanceof Error) return e.message; throw e; } };
  for (const c of vectors.fee_cases) eq(c.id, code(() => feeAtoms(c.receive, c.active_bps)), c.expected);
  for (const c of vectors.cap_cases) {
    eq(c.id, code(() => checkFeeCap(c.cap, c.active_bps)), c.expected);
    const order = { ...m, max_fee_bps: c.cap, max_qty_lots: '100', limit_price_ticks: '100' };
    if (c.expected === 'INTEGER_RANGE') { eq(c.id + ':sign-reject', code(() => sign('OrderV1', order, secretKey)), 'INTEGER_RANGE'); continue; }
    const signed = sign('OrderV1', order, secretKey);
    const snapshot = { ...vectors.decision_cases[0].input.snapshot, cap: c.cap, active_bps: c.active_bps };
    const result = decideOrder(signed.body, signed.signature, ctx, snapshot);
    eq(c.id + ':actual-signature', result.authentication, { status: 'PASS', code: 'OK' });
    eq(c.id + ':policy', result.snapshot_policy.code, c.expected === 'OK' ? code(() => feeAtoms('10000', c.active_bps)) === 'FEE_GE_RECEIVE' ? 'FEE_GE_RECEIVE' : 'OK' : c.expected);
  }
  // Specification-only injected preconditions, labelled separately from real crypto below.
  for (const c of vectors.decision_cases) eq('synthetic-spec:' + c.id, evaluateSnapshot(c.input.authentication_result, c.input.snapshot), c.expected);
  for (const c of vectors.api_errors) eq('api:' + c.code, apiError(c.code), c.expected);
  const v = signatures.positives[0], body = hexToBytes(v.canonical_hex), signature = hexToBytes(v.signature_hex);
  for (const c of vectors.registration_cases) {
    const reg = c.registered;
    const key = ctx.registeredKey!.bytes.slice();
    if (reg && reg.raw_key_ref !== 'submitted') key[0] ^= 1;
    const result = decideOrder(body, signature, { ...ctx, registeredKey: reg ? { type: 'key_type' in reg ? reg.key_type : undefined, bytes: key } : undefined }, null);
    eq('actual-registration:' + c.id, result.authentication, c.expected_registration === 'OK' ? { status: 'PASS', code: 'OK' } : c.expected_registration === 'NOT_CONNECTED' ? { status: 'NOT_CONNECTED', code: null } : { status: 'REJECTED', code: c.expected_registration });
  }
  const snap = { ...vectors.decision_cases[0].input.snapshot, q: '1', p: '1', cap: '25', active_bps: '25' } as Snapshot;
  for (const side of ['1', '2', '3']) for (const order_type of ['1', '2', '3']) {
    const order = { ...m, side, order_type, max_qty_lots: '100', limit_price_ticks: '100', max_fee_bps: '25' };
    const signedEnum = sign('OrderV1', order, secretKey);
    const enumSnap: Snapshot = { ...snap, q: '100', p: '100' };
    const result = decideOrder(signedEnum.body, signedEnum.signature, ctx, enumSnap);
    const id = `enum-${side}-${order_type}`;
    eq(id + ':authentication', result.authentication, { status: 'PASS', code: 'OK' });
    eq(id + ':policy', result.snapshot_policy, { status: side === '3' || order_type === '3' ? 'REJECTED' : 'PASS', code: side === '3' || order_type === '3' ? 'MARKET_LIMIT' : 'OK', source: 'SYNTHETIC', snapshot_id: enumSnap.id });
    eq(id + ':ack', result.ack, 'NOT_CONNECTED');
    eq(id + ':wal', result.wal_replay, 'NOT_RUN');
    eq(id + ':ledger', result.ledger, 'NOT_CONNECTED');
  }
  const signed = sign('OrderV1', { ...m, max_qty_lots: '1', limit_price_ticks: '1', max_fee_bps: '25' }, secretKey);
  const actual = decideOrder(signed.body, signed.signature, ctx, snap);
  eq('actual-crypto-tiny-fill', actual, vectors.decision_cases[1].expected);
  const corrupted = signed.signature.slice(); corrupted[0] ^= 1;
  eq('tampered-signature', decideOrder(signed.body, corrupted, ctx, snap).authentication, { status: 'REJECTED', code: 'INVALID_SIGNATURE' });
  eq('tampered-policy-not-run', decideOrder(signed.body, corrupted, ctx, snap).snapshot_policy.status, 'NOT_RUN');
  eq('missing-snapshot', decideOrder(signed.body, signed.signature, ctx, null).snapshot_policy.status, 'NOT_CONNECTED');
  eq('mismatched-snapshot', decideOrder(signed.body, signed.signature, ctx, { ...snap, cap: '10000' }).snapshot_policy.status, 'NOT_CONNECTED');
  for (const field of Object.keys(snap)) {
    const missing = { ...snap }; delete missing[field];
    eq('missing-field:' + field, decideOrder(signed.body, signed.signature, ctx, missing).snapshot_policy.status, 'NOT_CONNECTED');
  }
  eq('api-refuses-test-status', code(() => apiError('NOT_CONNECTED')), 'CONTEXT_MISMATCH');
  return checks;
}
