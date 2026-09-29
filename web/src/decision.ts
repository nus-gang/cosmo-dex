import { ContractError, fail, uint, type Message } from './codec.ts';
import { authenticate, checkFeeCap, feeAtoms, type VerificationContext } from './wallet.ts';

export type Verdict = { status: string; code: string | null };
export type Snapshot = Record<string, unknown>;
const required = ['id', 'source', 'height', 'expiry_height', 'epoch_matches', 'revoked', 'id_state', 'cumulative_ok', 'confirmed_balance_ok', 'q', 'p', 'active_bps', 'cap'];
const flags = ['epoch_matches', 'revoked', 'cumulative_ok', 'confirmed_balance_ok'];
function complete(s: Snapshot | null): s is Snapshot {
  return !!s && required.every(k => s[k] !== undefined && s[k] !== null)
    && typeof s.id === 'string' && s.id.length > 0 && s.source === 'SYNTHETIC'
    && flags.every(k => typeof s[k] === 'boolean') && ['NEW', 'CONFLICT'].includes(s.id_state as string);
}
export function outcome(fn: () => unknown): Verdict {
  try { fn(); return { status: 'PASS', code: 'OK' }; }
  catch (e) {
    if (!(e instanceof ContractError)) throw e;
    return e.code === 'NOT_CONNECTED' ? { status: 'NOT_CONNECTED', code: null } : { status: 'REJECTED', code: e.code };
  }
}
function policy(s: Snapshot): void {
  const height = uint(s.height, 64), expiry = uint(s.expiry_height, 64), q = uint(s.q, 64), p = uint(s.p, 64);
  uint(s.cap, 32);
  if (s.id_state === 'CONFLICT') fail('ID_CONFLICT');
  if (!s.epoch_matches) fail('EPOCH_MISMATCH');
  if (s.revoked) fail('ORDER_REVOKED');
  if (height >= expiry) fail('EXPIRED');
  if (q < 1n || q > 1000000n || p < 1n || p > 1000000n) fail('MARKET_LIMIT');
  checkFeeCap(s.cap, s.active_bps);
  feeAtoms((q * 1000n).toString(), s.active_bps as string);
  feeAtoms((q * p).toString(), s.active_bps as string);
  if (!s.cumulative_ok) fail('CUMULATIVE_QTY_EXCEEDED');
  if (!s.confirmed_balance_ok) fail('INSUFFICIENT_CONFIRMED_BALANCE');
}
// Synthetic policy evaluation alone is never evidence of authentication or admission.
export function evaluateSnapshot(authentication: Verdict, snapshot: Snapshot | null) {
  const result = {
    authentication,
    snapshot_policy: { status: 'NOT_RUN', code: null as string | null, source: 'SYNTHETIC', snapshot_id: snapshot?.id ?? null },
    ack: 'NOT_CONNECTED', wal_replay: 'NOT_RUN', ledger: 'NOT_CONNECTED',
  };
  if (authentication.status !== 'PASS') return result;
  Object.assign(result.snapshot_policy, complete(snapshot) ? outcome(() => policy(snapshot)) : { status: 'NOT_CONNECTED', code: null });
  return result;
}
export function decideOrder(body: Uint8Array, signature: Uint8Array, ctx: VerificationContext, snapshot: Snapshot | null) {
  let message: Message | undefined;
  const auth = outcome(() => { message = authenticate('OrderV1', body, signature, ctx); });
  // The synthetic policy must describe these signed fields and this observation.
  // No missing state is defaulted to an accepting value.
  if (auth.status === 'PASS' && complete(snapshot)) {
    const matches = snapshot.id === ctx.snapshotId && snapshot.height === ctx.height && snapshot.expiry_height === message!.expiry_height
      && snapshot.q === message!.max_qty_lots && snapshot.p === message!.limit_price_ticks
      && snapshot.cap === message!.max_fee_bps && ctx.epoch !== undefined
      && snapshot.epoch_matches === (message!.owner_epoch === ctx.epoch);
    if (!matches) return evaluateSnapshot(auth, { ...snapshot, source: null });
  }
  return evaluateSnapshot(auth, snapshot);
}
export function apiError(code: string, height: string | null = null) {
  if (!['INTEGER_RANGE', 'BPS_RANGE', 'FEE_CAP', 'FEE_GE_RECEIVE', 'ACCOUNT_KEY_UNREGISTERED', 'ACCOUNT_KEY_MISMATCH'].includes(code)) fail('CONTEXT_MISMATCH');
  if (height !== null) uint(height, 64);
  return { http_status: 400, body: { code, retryable: false, state: 'REJECTED', height } };
}
