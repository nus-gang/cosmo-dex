// Test-only independent JSONL adapter around the submitted TS implementation.
import { createInterface } from 'node:readline';
import { encode, decode, frame, hexToBytes as h, bytesToHex as hex } from '../../src/codec.ts';
import { ml_dsa65, verify, validateDevOrder, feeAtoms } from '../../src/wallet.ts';
import { decideOrder, evaluateSnapshot } from '../../src/decision.ts';
import { checkFeeCap } from '../../src/wallet.ts';
import { context } from '../../test/conformance.ts';
// noble 0.4.1 runtime supports FIPS context/randomizer arguments missing from its declarations.
const pure = ml_dsa65 as unknown as { sign: (sk: Uint8Array, msg: Uint8Array, ctx: Uint8Array, rnd: Uint8Array) => Uint8Array; verify: (pk: Uint8Array, msg: Uint8Array, sig: Uint8Array, ctx: Uint8Array) => boolean };
for await (const line of createInterface({input:process.stdin})) {
 const r=JSON.parse(line); let o:any={code:'OK'};
 try { switch(r.Op) {
 case 'sign': {const k=ml_dsa65.keygen(h(r.Seed));try{o.pk=hex(k.publicKey);o.sig=hex(pure.sign(k.secretKey,h(r.Msg),h(r.Context??''),new Uint8Array(32)));}finally{k.secretKey.fill(0);}break;}
 case 'verify': try{o.valid=pure.verify(h(r.PK),h(r.Msg),h(r.Sig),h(r.Context??''));}catch{o.valid=false;}break;
 case 'encode': {const b=encode(r.Name,r.API);o.wire=hex(b);o.msg=hex(frame(r.Domain,b));break;}
 case 'decode': o.api=decode(r.Name,h(r.Wire));break;
 case 'fee-decimal': o.fee=feeAtoms(r.Receive,r.Rate);break;
 case 'cap': checkFeeCap(r.Cap,r.Rate);break;
 case 'snapshot': o.decision=evaluateSnapshot(r.Auth,r.Snapshot);break;
 case 'fee': o.fee=feeAtoms(r.Receive,String(r.BPS));break;
 case 'decision': case 'policy': {const m=decode(r.Name,h(r.Wire)),c=context(r.Name,m,h(r.PK));c.height=String(r.Height??999);if(r.RegisteredKeyType)c.registeredKey!.type=r.RegisteredKeyType;if(r.RegisteredKey)c.registeredKey!.bytes=h(r.RegisteredKey);if(r.Expected)c.expected={...c.expected,...r.Expected};if(r.Unregistered)c.registeredKey=undefined;if(r.MissingKeyType)delete (c.registeredKey as any).type;c.snapshotId=r.SnapshotID;if(r.Epoch!==undefined)c.epoch=String(r.Epoch);if(r.Op==='decision'){o.decision=decideOrder(h(r.Wire),h(r.Sig),c,r.Snapshot);break;}verify(r.Name,h(r.Wire),h(r.Sig),c);if(r.Name==='OrderV1')validateDevOrder(m,String(r.BPS??0));break;}
 default: throw Error('unsupported');}
 }catch(e){o={code:(e as Error).message};}
 console.log(JSON.stringify(o));
}
