import { ml_dsa65, owner, address, sign, sha256 } from '../src/wallet.ts';
import { decode, uint } from '../src/codec.ts';
import { base64, envelope, join, bytes, hex, type Input } from './direct-codec.ts';
import type { Context } from './state.ts';
export class LocalKey {
  #secret: Uint8Array; #closed=false;
  readonly publicKey: Uint8Array; readonly owner: string; readonly address: string;
  constructor() {
    const seed=crypto.getRandomValues(new Uint8Array(32));
    try {const k=ml_dsa65.keygen(seed);this.#secret=k.secretKey;this.publicKey=k.publicKey;}finally{seed.fill(0);}
    this.owner=base64.encode(owner(this.publicKey));this.address=address(this.publicKey);
  }
  destroy(){this.#secret.fill(0);this.#closed=true;}
  challenge(wire: string, ctx: Context, origin: string) {
    if(this.#closed)throw Error('KEY_CLOSED');
    const m=decode('WalletChallengeV1',base64.decode(wire)),now=BigInt(Math.floor(Date.now()/1000));
    if(!['http://127.0.0.1:5173','http://localhost:5173'].includes(origin)||m.protocol_version!=='1'||m.owner!==this.owner||m.chain_id!==ctx.chain_id||m.genesis_hash!==ctx.genesis_hash||m.server_origin!==origin||m.audience!=='exchange-api')throw Error('CHALLENGE_CONTEXT');
    const start=uint(m.issued_at,64),end=uint(m.expiry_time,64);
    if(now<start||now>=end||end<=start||end-start>120n)throw Error('CHALLENGE_EXPIRED');
    const s=sign('WalletChallengeV1',m,this.#secret);
    if(base64.encode(s.body)!==wire)throw Error('NON_CANONICAL');
    return {wire_base64:wire,signature_base64:base64.encode(s.signature)};
  }
  direct(input: Input) {
    if(this.#closed)throw Error('KEY_CLOSED');
    const e=envelope(input,this.publicKey);
    const raw=join(bytes(1,e.body),bytes(2,e.auth),bytes(3,ml_dsa65.sign(this.#secret,e.signDoc)));
    if(raw.length>16384)throw Error('TX_SIZE');
    return {tx_bytes:base64.encode(raw),tx_hash:hex(sha256(raw))};
  }
}
