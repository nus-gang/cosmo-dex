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
    if (url === '/s1/network') return Response.json({ genesis_hash: '11'.repeat(32), chain_id: 'nus-s1-dev-1', denom: 'DEVQUOTE', decimals: '6', gas_denom: 'DEVGAS' });
    if (url.startsWith('/s1/accounts/')) return Response.json({ owner: w.keys[0].owner, public_key_type: 'ML-DSA-65', public_key_base64: w.publicKeys()[0], account_number: '7', sequence: '3', epoch: '0', bank_atoms: '1000000000', exchange_atoms: '60000000', gas_atoms: '1000000', observed_height: '12', state: 'COMMITTED' });
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
