# 브라우저 ChainPort 인증 API 보완 요청

2026-10-07 · NUS-73 → NUS-72 원 Wallet 구현·CTO→Security 재심사.

승인 web/s3/client.ts의 LocalClient는 #token/#request를 private으로 보관한다. ChainPort는 constructor에서 주입받고 account(address), broadcast(tx_bytes), result(tx_hash)만 호출한다. 현재 SRE HTTP 세 경로는 승인 Rest의 같은 Bearer 세션을 요구한다. 외부 port가 같은 fetch를 사용해도 LocalClient가 내부 요청에만 추가한 Authorization은 전달되지 않는다.

재현: `node --experimental-strip-types --test ops/s3-local/browser-auth-gap.test.ts` (NUS-73 checkout). 신규 1 PASS/0 FAIL. 실제 일회성 ML-DSA challenge 서명과 LocalClient 로그인/출금 판단, 합성 HTTP 응답을 사용한다. 내부 capabilities/account에는 token이 붙고 외부 chain/account에는 없으며 401로 거절된다. 서명 TX/history 생성0·방송0. API 공백 재현 성공이지 브라우저 통합 PASS가 아니다.

Wallet 요청: token을 공개하거나 SRE 별도 세션 저장소를 만들지 않고, 승인 client의 동일 private session/두 opt-in/계정 generation에 결합된 좁은 ChainPort HTTP 배선을 제공한다. 예: client 내부 authenticated request를 고정 route adapter에 전달하는 factory. 구체 API는 Wallet 소유이며 기존 직접 서명/UNKNOWN/금액 의미는 보존한다.

고정 SRE 경로:
- GET /dev-local/v1/chain/account, 본문 없음, owner는 인증 세션. 응답 DirectAccount이며 원 received_at_unix_ms 유지.
- POST /dev-local/v1/chain/broadcast, {"tx_bytes":"canonical base64"}. 기존 helper의 owner/서명 검증 뒤 한 번 전송; 응답은 UNKNOWN, 확정으로 해석 금지.
- POST /dev-local/v1/chain/result, {"tx_hash":"lowercase hex64"}. 검증된 원 TX/Context/height/code/state; 오류는 UNKNOWN 유지.

필수 검증: 같은 세션 사용, 두 opt-in 누락 IO0, select/destroy·계정 변경 중 지연 응답 폐기, stale/철회 후 방송0, origin/상대 route 고정, no-store/redirect error/2초 상한, UNKNOWN 후 자동 재방송0. 토큰·키 로그/응답 노출0. 경제·cap·공통 protocol 변경이 필요하면 기존 CTO 계약 경로로 반환한다.

검토된 새 Wallet 후보와 CTO→Security 완료 이벤트 후 SRE가 통합·브라우저 build·manifest를 계속한다. 기존 승인 candidate를 새 파일에 재사용하지 않는다. 서비스/START/RPC/방송0·runtime pin 미발급·DEV NOT_RUN·€0. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED와 표준 부모 blocker 유지.
