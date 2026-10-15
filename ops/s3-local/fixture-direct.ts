// Offline test-only signer using published fixture keys; never service keys.
import {readFileSync,writeFileSync} from 'node:fs';
import {ml_dsa65,sha256} from '../../web/src/wallet.ts';
import {envelope,bytes,join,base64,hex,address} from '../../web/s3/direct-codec.ts';
const [input,output]=process.argv.slice(2); if(!input||!output||process.argv.length!==4)throw Error('ARGS');
const v=JSON.parse(readFileSync(input,'utf8'));
const keys=JSON.parse(readFileSync(new URL('../../protocol/s3/vectors/test-keys.json',import.meta.url),'utf8'));
if(![0,1].includes(v.index))throw Error('FIXTURE_INDEX');
const seed=Uint8Array.from(Buffer.from(keys[v.index].test_seed_hex,'hex'));
const k=ml_dsa65.keygen(seed); seed.fill(0);
try {
 const e=envelope({operation:v.operation,denom:'DEVBASE',owner:address(k.publicKey),amount_atoms:'1',request_id:'ab'.repeat(32),expected_epoch:'0',expiry_height:'110',genesis_hash:v.context.genesis_hash,chain_id:v.context.chain_id,account_number:'0',sequence:'0',fee_atoms:'1',gas_limit:'1000000'},k.publicKey);
 const raw=join(bytes(1,e.body),bytes(2,e.auth),bytes(3,ml_dsa65.sign(k.secretKey,e.signDoc)));
 writeFileSync(output,JSON.stringify({tx_bytes:base64.encode(raw),tx_hash:hex(sha256(raw))}),{flag:'wx',mode:0o600});
}finally{k.secretKey.fill(0);}
