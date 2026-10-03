// Test-only browser signer. Keys remain in this closure, never cross page.evaluate.
import { ml_dsa65 } from '@noble/post-quantum/ml-dsa';
import { sha256 } from '@noble/hashes/sha256';
import { base64, bech32 } from '@scure/base';
const keys = [0, 1].map(() => {
  const seed = crypto.getRandomValues(new Uint8Array(32));
  try { return ml_dsa65.keygen(seed); } finally { seed.fill(0); }
});
const text = new TextEncoder();
const join = (...xs) => Uint8Array.from(xs.flatMap(x => [...x]));
function vi(n) { n = BigInt(n); const a = []; do { let b = Number(n & 127n); n >>= 7n; if (n) b |= 128; a.push(b); } while (n); return Uint8Array.from(a); }
const bytes = (t, b) => join(vi(t * 8 + 2), vi(b.length), b);
const str = (t, s) => bytes(t, text.encode(s));
const num = (t, n) => BigInt(n) === 0n ? new Uint8Array() : join(vi(t * 8), vi(n));
const any = (url, value) => join(str(1, url), bytes(2, value));
const hex = b => [...b].map(v => v.toString(16).padStart(2, '0')).join('');
const unhex = s => Uint8Array.from(s.match(/../g), b => parseInt(b, 16));
const owner = pk => bech32.encode('nus', bech32.toWords(sha256(pk).slice(0, 20)));
window.accountTest = {
  publicKeys: keys.map(k => base64.encode(k.publicKey)),
  owners: keys.map(k => owner(k.publicKey)),
  async transact({api, user, denom, operation, amount}) {
    const get = async path => { const r = await fetch(api + path); if (r.status !== 200) throw Error('HTTP ' + r.status); return r.json(); };
    const key = keys[user], address = owner(key.publicKey);
    const before = await get('/s2/accounts/' + address);
    if (!before.signing_ready || before.state !== 'COMMITTED' || before.owner !== address ||
        before.public_key_base64 !== base64.encode(key.publicKey)) throw Error('account binding');
    const rid = crypto.getRandomValues(new Uint8Array(32));
    const msg = join(str(1,address), str(2,denom), str(3,amount), bytes(4,rid), str(5,before.owner_epoch),
      str(6,String(BigInt(before.observed_height)+100n)), bytes(7,unhex(before.context.genesis_hash)));
    const body = bytes(1,any('/nus.exchange.v1.Msg'+operation,msg));
    const signer = join(bytes(1,any(before.public_key_type,bytes(1,key.publicKey))), bytes(2,bytes(1,num(1,1))), num(3,before.sequence));
    const auth = join(bytes(1,signer),bytes(2,join(bytes(1,join(str(1,before.gas_denom),str(2,'1000'))),num(2,'500000'))));
    const doc = join(bytes(1,body),bytes(2,auth),str(3,before.context.chain_id),num(4,before.account_number));
    const sig = ml_dsa65.sign(key.secretKey,doc), raw = join(bytes(1,body),bytes(2,auth),bytes(3,sig));
    if (!ml_dsa65.verify(key.publicKey, doc, sig)) throw Error('signature');
    const response = await fetch(api+'/s1/txs',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({tx_bytes:base64.encode(raw)})});
    if(response.status!==202) throw Error('submit');
    return {before, submitted:await response.json(), tx_bytes:base64.encode(raw), tx_hash:hex(sha256(raw)).toUpperCase(), request_id:hex(rid), signature_verified:true};
  }
};
