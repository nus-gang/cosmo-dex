import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { ml_dsa65 } from '@noble/post-quantum/ml-dsa';
import { envelope, hex, atoms, display, SessionKey, type Input } from './direct.ts';
import { WalletClient } from './client.ts';
const vec = JSON.parse(readFileSync(new URL('./direct-vectors.json', import.meta.url), 'utf8'));
const fromHex = (s: string) => Uint8Array.from(Buffer.from(s, 'hex'));
for (const c of vec.cases) test(`Go golden ${c.id}: message/body/auth/SignDoc and signature`, () => {
  const e = envelope({ ...c, operation: c.type_url.endsWith('MsgDeposit') ? 'DEPOSIT' : 'WITHDRAW' }, fromHex(vec.public_key_hex));
  assert.equal(hex(e.message), c.message_hex); assert.equal(hex(e.body), c.body_hex); assert.equal(hex(e.auth), c.auth_info_hex); assert.equal(hex(e.signDoc), c.sign_doc_hex);
  assert.ok(ml_dsa65.verify(fromHex(vec.public_key_hex), e.signDoc, fromHex(c.signature_hex)));
});
test('decimal input rejects rounding, exponent, leading zeros and overflow', () => {
  assert.equal(atoms('100'), '100000000'); assert.equal(atoms('40'), '40000000'); assert.equal(display('60000000'), '60.000000');
  for (const x of ['0', '-1', '1e2', '01', '0.0000001', '1000000.000001', ' 1', '1.']) assert.throws(() => atoms(x));
});
test('random session keys differ; closed session cannot sign', () => {
  const a = new SessionKey(), b = new SessionKey(); assert.notEqual(a.owner, b.owner);
  a.destroy(); b.destroy(); assert.throws(() => a.sign({} as Input), /SESSION_CLOSED/);
});
function fixture() {
  let posts = 0; let txResult: unknown = null; let failPost = true;
  const w = new WalletClient((async (path, options) => {
    const url = String(path);
    if (url === '/s1/network') return Response.json({ genesis_hash: '11'.repeat(32), chain_id: 'nus-s1-dev-1', denom: 'DEVQUOTE', decimals: '6', gas_denom: 'DEVGAS', observed_height: '12' });
    if (url.startsWith('/s1/accounts/')) return Response.json({ owner: w.keys[0].owner, public_key_type: '/cosmos.crypto.mldsa65.PubKey', public_key_base64: w.publicKeys()[0], account_number: '7', sequence: '3', epoch: '0', bank_atoms: '1000000000', exchange_atoms: '60000000', gas_atoms: '1000000', observed_height: '12', state: 'COMMITTED' });
    if (options?.method === 'POST') { posts++; assert.deepEqual(Object.keys(JSON.parse(String(options.body))), ['tx_bytes']); if (failPost) throw Error('lost response'); return Response.json({}, { status: 202 }); }
    return txResult ? Response.json(txResult) : new Response('{}', { status: 404 });
  }) as typeof fetch);
  w.bindGenesis('11'.repeat(32)); return { w, posts: () => posts, result: (x: unknown) => { txResult = x; }, allowPost: () => { failPost = false; } };
}
test('lost response keeps same hash and locks next signature; 404 cannot fail TX', async () => {
  const f = fixture(); const pending = f.w.submit(0, 'DEPOSIT', '100000000');
  await assert.rejects(f.w.submit(0, 'WITHDRAW', '40000000'));
  const e = await pending; assert.equal(e.state, 'SUBMISSION_UNKNOWN'); assert.equal(f.posts(), 1);
  const hash = e.tx_hash; await f.w.resolve(e); assert.equal(e.state, 'SUBMISSION_UNKNOWN'); assert.equal(e.tx_hash, hash);
  await assert.rejects(f.w.submit(0, 'DEPOSIT', '1')); assert.equal(f.posts(), 1);
  f.result({ tx_hash: hash, state: 'COMMITTED', height: '13', code: '0' }); await f.w.resolve(e); assert.equal(e.state, 'COMMITTED'); f.w.close();
});
test('wrong hash, zero height, inconsistent result cannot confirm', async () => {
  const f = fixture(); f.allowPost(); const e = await f.w.submit(0, 'DEPOSIT', '1');
  for (const r of [{ tx_hash: 'A'.repeat(64), height: '13', code: '0' }, { tx_hash: e.tx_hash, height: '0', code: '0' }, { tx_hash: e.tx_hash, height: '13', code: '1' }]) { f.result({ ...r, state: 'COMMITTED' }); await f.w.resolve(e); assert.equal(e.state, 'SUBMISSION_UNKNOWN'); }
  f.result({ tx_hash: e.tx_hash, height: '13', code: '5', state: 'REJECTED_FINAL' }); await f.w.resolve(e); assert.equal(e.state, 'REJECTED_FINAL'); f.w.close();
});
test('excess withdrawal never signs/submits', async () => { const f = fixture(); await assert.rejects(f.w.submit(0, 'WITHDRAW', '60000001'), /INSUFFICIENT_BALANCE/); assert.equal(f.posts(), 0); assert.equal(f.w.history.length, 0); f.w.close(); });
function freshnessFixture() {
  let height = '11', network = '11', balance = '61000000', delayed = false;
  let release: (() => void) | undefined;
  const w = new WalletClient((async path => {
    if (String(path) === '/s1/network') return Response.json({ genesis_hash: '11'.repeat(32), chain_id: 'nus-s1-dev-1', denom: 'DEVQUOTE', decimals: '6', gas_denom: 'DEVGAS', observed_height: network });
    const index = String(path).endsWith(w.keys[0].owner) ? 0 : 1;
    const response = Response.json({ owner: w.keys[index].owner, public_key_type: '/cosmos.crypto.mldsa65.PubKey', public_key_base64: w.publicKeys()[index], account_number: '7', sequence: '3', epoch: '0', bank_atoms: '1000000000', exchange_atoms: balance, gas_atoms: '1000000', observed_height: height, state: 'COMMITTED' });
    if (delayed) { delayed = false; await new Promise<void>(r => { release = r; }); }
    return response;
  }) as typeof fetch);
  w.bindGenesis('11'.repeat(32));
  return { w, set: (h: string, n = h, b = balance) => { height = h; network = n; balance = b; }, delay: () => { delayed = true; }, release: () => release!(), ready: () => !!release };
}
test('height 11 C=61 cannot regress to height 9 C=60; stale input cannot sign', async () => {
  const f = freshnessFixture(); assert.equal((await f.w.account(0)).exchange_atoms, '61000000');
  f.set('9', '11', '60000000'); await assert.rejects(f.w.account(0), /STALE_ACCOUNT/);
  await assert.rejects(f.w.submit(0, 'WITHDRAW', '1'), /STALE_ACCOUNT/); assert.equal(f.w.history.length, 0);
  assert.equal((await f.w.account(1)).observed_height, '9'); // Each owner has its own watermark.
  f.set('12'); assert.equal((await f.w.account(0)).observed_height, '12'); f.w.close();
});
test('network/account skew is labelled, not equality-gated; exact bigint heights', async () => {
  const f = freshnessFixture(); f.set('9007199254740993', '9007199254740994');
  assert.equal((await f.w.account(0)).lag_blocks, '1');
  f.set('9007199254740995', '9007199254740994'); assert.equal((await f.w.account(0)).lag_blocks, '0');
  f.set('9007199254740994'); await assert.rejects(f.w.account(0), /STALE_ACCOUNT/); f.w.close();
});
test('confirmed receipt provides a floor even before next account refresh', async () => {
  const f = freshnessFixture();
  f.w.history.push({ input: { owner: f.w.keys[0].owner } as Input, tx_bytes: '', tx_hash: 'A'.repeat(64), state: 'COMMITTED', height: '12' });
  await assert.rejects(f.w.account(0), /STALE_ACCOUNT/); f.set('12'); await f.w.account(0); f.w.close();
});
test('out-of-order completion checks watermark at response time; closed session rejects pending read', async () => {
  const f = freshnessFixture(); f.set('9'); f.delay(); const old = f.w.account(0);
  while (!f.ready()) await new Promise(r => setTimeout(r, 0));
  f.set('11'); await f.w.account(0); f.release(); await assert.rejects(old, /STALE_ACCOUNT/);
  const g = freshnessFixture(); g.delay(); const pending = g.w.account(0);
  while (!g.ready()) await new Promise(r => setTimeout(r, 0));
  g.w.close(); g.release(); await assert.rejects(pending, /SESSION_CLOSED/); f.w.close();
});
