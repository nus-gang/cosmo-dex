import { readFileSync } from 'node:fs';
import { encode, decode, frame, atoms, fromAtoms, hexToBytes as unhex, bytesToHex as hex } from '../../../web/src/codec.ts';
import { ml_dsa65, feeAtoms, checkFeeCap } from '../../../web/src/wallet.ts';
import { evaluateSnapshot, apiError, decideOrder } from '../../../web/src/decision.ts';
const requests=JSON.parse(readFileSync(0,'utf8'));
console.log(JSON.stringify(requests.map((r:any)=>{
 let actual:any;
 try{
 switch(r.op){
 case 'rc4': {
 const n=r.native,c=n.context,reg=c.registered;
 const ctx:any={expected:{...c,fee_asset_policy_id:'RECEIVE_ASSET_V1'},registeredKey:{bytes:unhex(reg.raw_key_hex),type:reg.key_type},snapshotId:c.snapshot_id,height:c.height,epoch:c.epoch};
 actual=decideOrder(unhex(n.wire_hex),unhex(n.signature_hex),ctx,n.snapshot??null);break;
 }
 case 'crypto': try{actual=(ml_dsa65 as any).verify(unhex(r.pk),unhex(r.input),unhex(r.signature),unhex(r.context??""));}catch{actual=false;}break;
 case 'frame':actual=hex(frame(r.domain,unhex(r.wire)));break;
 case 'atoms':actual=hex(atoms(r.api));break;
 case 'atoms_decode':actual=fromAtoms(unhex(r.wire));break;
 case 'encode':actual=hex(encode(r.message,r.api));break;
 case 'decode':decode(r.message,unhex(r.wire));actual='CANONICAL';break;
 case 'fee':actual=feeAtoms(r.receive,r.rate);break;
 case 'cap':checkFeeCap(r.cap,r.rate);actual='OK';break;
 case 'decision':actual=evaluateSnapshot(r.auth,r.snapshot);break;
 case 'api':actual=apiError(r.code);break;
 default:throw Error('unsupported op');
 }
 }catch(e){actual=(e as Error).message;}
 return {id:r.id,actual};
})));
