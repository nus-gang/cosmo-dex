// Test-only malformed envelopes. Uses pinned noble ML-DSA; no production key input.
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
const require=createRequire(process.env.NUS_TEST_DEPENDENCIES?process.env.NUS_TEST_DEPENDENCIES+'/package.json':fileURLToPath(new URL('../../web/package.json',import.meta.url)));
const {ml_dsa65}=require('@noble/post-quantum/ml-dsa');
const {sha256}=require('@noble/hashes/sha256');
const {bech32}=require('@scure/base');
const i=JSON.parse(readFileSync(0,'utf8'));
const join=(...a)=>Buffer.concat(a.map(x=>Buffer.from(x)));
function v(n){n=BigInt(n);if(n<0n)throw Error('negative varint');let a=[];do{let b=Number(n&127n);n>>=7n;if(n)b|=128;a.push(b);}while(n);return Buffer.from(a);}
const b=(t,x)=>join(v(t*8+2),v(x.length),x), s=(t,x)=>b(t,Buffer.from(x)), n=(t,x)=>BigInt(x)===0n?Buffer.alloc(0):join(v(t*8),v(x));
const any=(u,x)=>join(s(1,u),b(2,x));
const seed=new Uint8Array(32);seed[0]=(i.key_index??0)+1;
const k=ml_dsa65.keygen(seed);seed.fill(0);
const owner=i.owner??bech32.encode('nus',bech32.toWords(sha256(k.publicKey).slice(0,20)));
let msg=join(s(1,owner),s(2,i.denom??'DEVQUOTE'),s(3,i.amount??'1'),b(4,Buffer.from(i.request_id,'hex')),s(5,i.epoch??'0'),s(6,i.expiry??'1000000000'),b(7,Buffer.from(i.genesis_hash,'hex')));
let url='/nus.exchange.v1.Msg'+(i.op??'Deposit');
if(i.bank_send){url='/cosmos.bank.v1beta1.MsgSend';msg=join(s(1,owner),s(2,i.destination),b(3,join(s(1,'DEVQUOTE'),s(2,'1'))));}
const body=b(1,any(url,msg));
const signer=join(b(1,any('/cosmos.crypto.mldsa65.PubKey',b(1,k.publicKey))),b(2,b(1,n(1,1))),n(3,i.sequence));
const fee=join(b(1,join(s(1,i.fee_denom??'DEVGAS'),s(2,i.fee??'1000'))),n(2,i.gas??'500000'),i.payer?s(3,i.payer):Buffer.alloc(0),i.granter?s(4,i.granter):Buffer.alloc(0));
const auth=join(b(1,signer),b(2,fee));
const doc=join(b(1,body),b(2,auth),s(3,i.chain_id??'nus-s1-dev-1'),n(4,i.account_number));
const sig=ml_dsa65.sign(k.secretKey,doc);k.secretKey.fill(0);if(i.tamper)sig[0]^=1;
process.stdout.write(join(b(1,body),b(2,auth),b(3,sig)).toString('base64'));
