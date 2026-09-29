# S0-F TS 지갑 계약

기준선: S0-A `889fda0c7181a696b4eb2a2649508c6192af8406` (rc2, 계약 단계 Security→QA 승인). `protocol/v1/`은 수정하지 않고 직접 소비한다. 이전 M0의 JSON 의도 서명은 이 모듈의 주문·취소·인증 경로에서 사용하지 않는다.

```sh
cd web
npm ci --ignore-scripts
npm run build
npm test
node --experimental-strip-types scripts/vectors.ts ../protocol/v1/vectors/signatures.json
CHROME_BIN='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' npm run test:browser
# Linux: CHROME_BIN=/usr/bin/google-chrome
```

Node >=22.18 (시험 24.21.0), Chrome 실제 실행 154.0.8037.58. `dist/demo.html`은 파일로 여는 검증 전용 자체 화면이다. 외부 CDN·서버·API 없이 동작한다. 제품 UI 채택 결정은 아니다. Chrome 설치 경로를 명시해야 하며 Node 테스트만으로 브라우저 PASS라고 하지 않는다.

- `src/codec.ts`: schema 기반 정규 encode/decode, strict JSON 사전 파서, U32/U64/U128, atoms 16-byte BE, domain frame.
- `src/wallet.ts`: pure ML-DSA-65 / empty FIPS context, owner SHA256(raw pk)[:20], canonical `nus` Bech32, 확정 등록 키·context·epoch·만료 검증. Order/Cancel/WalletChallenge만 서명 가능하다.
- `test/conformance.ts`: 공통 서명 긍정 3개/부정 35개, amount 17개, 메시지 14개, wire 18개 및 추가 경계. 전체 **204 assertions**, Node와 Chrome에서 동일 suite 실행.
- `scripts/vectors.ts`: SRE argv runner. stdout은 단일 JSON, `contract_revision`은 A SHA, `vectors_sha256`은 입력 signatures.json 원문 SHA256. 38개 결과의 `id/sign_bytes_hex/valid`를 출력하고 입력/기대값 불일치 시 nonzero. 추가로 공개 서명 벡터를 `evidence/ts-generated.json`에 생성한다. Go/Rust runner와 필드·revision 표현 통합은 SRE/CTO 조정 대상이며 이번 결과는 독립 3언어 실행 매트릭스가 아니다.
- `.github/workflows/wallet-contract.yml`: F 전용 Node·Chrome 실행. SRE의 기존 workflow를 수정하지 않았다. 향후 S0 공통 manifest에는 `cwd=web`, `argv=["node","--experimental-strip-types","scripts/vectors.ts","{vectors}"]`를 연결할 수 있다.

`verify`는 요청자가 제공한 config를 신뢰하지 않는다. 호출자는 확정 계정/설정에서 얻은 `VerificationContext`를 공급해야 한다. 반환값은 인증된 메시지이며 서버의 완전한 주문 접수 성공이 아니다. 중복 ID·epoch 최신성·철회·잔고/누적 체결·nonce 원자 소비·WAL 및 모든 상태 전이는 서버 책임이다. WalletChallenge 검증 뒤에는 서버의 nonce 원자 소비가 별도로 필요하며 이 함수를 단독 로그인 endpoint로 노출해서는 안 된다. DEV 시장 제한/fee 함수도 별도 명시적 호출이다. m0-crypto fixture를 DEV 업무 승인으로 해석하지 않는다.

TransferStableV1의 canonical bytes/payment hash만 지원한다. 실제 SDK SIGN_MODE_DIRECT TX 서명·등록 계정 조회·REST/WS·체인 송금/입출금·지속 백업은 **NOT_CONNECTED/NOT_IMPLEMENTED**다. 기존 M0 모의 API의 성공 결과는 제품 통합 PASS로 승계하지 않는다. 화면에는 잠정/확정/정정/불확실의 의미와 이 경계를 명시했다.

시험용 공개 KAT seed는 기존 protocol 벡터의 합성 입력이다. 새로 생성한 개인키/seed는 메모리에만 두며 파일·서버·로그·문서에 기록하지 않는다. best-effort buffer wipe는 JS 런타임의 모든 복사본 소거를 보장하지 않는다. `dist/demo.html`에는 테스트 suite와 공개 KAT가 들어 있으므로 운영 지갑으로 배포하지 않는다.
