import { runConformance, fromFields, context } from '../test/conformance.ts';
import vectors from '../../protocol/v1/vectors/signatures.json' with { type: 'json' };
import { ml_dsa65, sign, verify, owner, address, recoveryProbe } from './wallet.ts';
import { base64 } from '@scure/base';
const result = runConformance();
(globalThis as unknown as { conformance: unknown }).conformance = result;
document.querySelector('#result')!.textContent = `실제 브라우저 검증: ${result.passed}개 PASS`;
let key: ReturnType<typeof ml_dsa65.keygen> | undefined;
const status = (s: string) => { document.querySelector('#status')!.textContent = s; };
document.querySelector('#create')!.addEventListener('click', () => {
  key?.secretKey.fill(0); const seed = crypto.getRandomValues(new Uint8Array(32));
  try { key = ml_dsa65.keygen(seed); status('모의 키 생성: ' + address(key.publicKey)); }
  finally { seed.fill(0); }
});
document.querySelector('#sign')!.addEventListener('click', () => {
  if (!key) return status('먼저 모의 키를 생성하세요.');
  const m = { ...fromFields('OrderV1', vectors.positives[0].fields), owner: base64.encode(owner(key.publicKey)), owner_pubkey: base64.encode(key.publicKey) };
  const s = sign('OrderV1', m, key.secretKey);
  verify('OrderV1', s.body, s.signature, context('OrderV1', m, key.publicKey));
  status(`공통 OrderV1 서명·검증 성공 (${s.signature.length} bytes). 전송 없음.`);
});
document.querySelector('#recover')!.addEventListener('click', () => status(recoveryProbe() ? '별도 모의 키의 메모리 내 복구 검증 성공. 파일 백업·현재 키 복원 아님.' : '복구 실패'));
addEventListener('pagehide', () => { key?.secretKey.fill(0); key = undefined; });
