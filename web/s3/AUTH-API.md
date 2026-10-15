# L-R 인계: 같은 private 세션의 사용자 서명 HTTP 연결

[NUS-73 요청](/NUS/issues/NUS-73#document-browser-auth-api-gap) revision `72f93221-300a-490b-ae95-072e32f10bac`를 반영했다. 출발 후보는 Wallet 승인 `6077a8397366a86d49b8e17eb12f988b8c21467f`이다.

```ts
const client = LocalClient.authenticated(context, window.fetch.bind(window), enabled, acknowledge);
client.select(tabKey);
await client.login('http://127.0.0.1:5173');
// mount(root, client, tabKeys, origin)이 DIRECT 예치/출금과 ORDER/CANCEL을 같은 tabKey로 실행한다.
```

`transport`는 신뢰하는 같은 origin의 fetch다. 승인 loopback listener와 브라우저 origin/CSP는 L-R이 배선·검토한다. factory는 사용자 지정 URL/route/헤더 콜백을 받지 않는다. 두 opt-in의 기본값은 false이며 누락 시 인증 포함 IO0이다. origin은 기존 두 loopback origin만 받는다.

| 작업 | 고정 상대 경로 | 입력 | 응답 처리 |
|---|---|---|---|
| 직접 계정 조회 | GET `/dev-local/v1/chain/account` | 본문·query 없음. 서버가 세션 owner 결정 | 원 DirectAccount, 원 received_at_unix_ms 유지. 기존 키/Context/높이/정수/gas/2초 검증 |
| 직접 예치·출금 방송 | POST `/dev-local/v1/chain/broadcast` | `{tx_bytes}` canonical base64 | 같은 tab key 사용자 서명 후 history UNKNOWN을 먼저 잠금. HTTP 성공도 UNKNOWN |
| 결과 조회 | POST `/dev-local/v1/chain/result` | `{tx_hash}` lowercase hex64 | 원 TX/Context/높이/code/state를 기존 규칙으로 대조. 오류·미발견은 UNKNOWN |
| 서명 주문 제출 | POST `/dev-local/v1/orders` | `{context,wire_base64,signature_base64}` exact `OrderV1` | 공개 계정 receipt의 kind/request_hash를 원 sign frame에 결합. 접수는 chain COMMITTED가 아님 |
| 서명 취소 제출 | POST `/dev-local/v1/cancels` | `{context,wire_base64,signature_base64}` exact `CancelV1` | 화면의 본인 잔량 order_id/order_hash만 같은 tab key로 서명 |
| 주문 원 결과 조회 | POST `/dev-local/v1/receipts/orders` | 최초 주문 제출 body와 byte-equivalent JSON 필드 | 새 ID·wire·서명·효과 없이 최초 공개 receipt 조회 |
| 취소 원 결과 조회 | POST `/dev-local/v1/receipts/cancels` | 최초 취소 제출 body와 byte-equivalent JSON 필드 | 새 nonce·wire·서명·효과 없이 최초 공개 receipt 조회 |

private adapter가 기존 `#request`와 동일 bearer를 사용한다. 공개 token getter, public request API, 별도 인증 저장소, 서버 signer를 추가하지 않았다. ChainPort 인스턴스도 private이다. 기존 constructor 주입 방식은 신뢰 component 시험용으로 유지하며 public `.chain` 접근 대신 기존 주입 참조를 사용한다.

`revokeSession()`은 키를 유지하고 인증·capability·projection을 초기화한다. select/destroy/revokeSession/재로그인은 generation을 바꾸고 진행 중 fetch를 abort한다. 요청은 헤더 수신과 JSON 파싱 완료 뒤 현재 generation, abort, 실제 경과 2초를 다시 확인한다. 모든 요청에 no-store, redirect:error, 2초 AbortSignal이 적용된다. 현재 세션 401/403은 철회하며 과거 세션의 오류는 새 세션에 영향이 없다. 새 login 없이 refresh/출금은 열리지 않는다. 서버가 철회 사실을 아직 전달하지 않은 경우의 최종 요청 허용 판정은 기존 서버 세션 검증 책임이다.

계정 조회를 기다리는 동안 계정 전환·철회·단절·보류가 발생하면 DIRECT·ORDER·CANCEL 서명 전에 거절한다. 이미 방송/제출한 요청은 abort만으로 무효과라고 판정할 수 없으므로 DIRECT는 UNKNOWN과 TX bytes/hash, ORDER/CANCEL은 원 wire/signature/request_hash를 보존한다. DIRECT resolve와 signed-result query는 현재 선택된 owner와 요청 generation에 한정한다. 다음 로그인 후 같은 owner가 명시적으로 원 결과를 조회할 수 있으며 자동 새 TX·자동 재방송·자동 재서명은 없다.

## 재현 및 결과

web 디렉터리에서 기존 설치 도구로 실행:

```sh
node --experimental-strip-types --test s3/component.test.ts s3/auth-api.test.ts
node_modules/.bin/tsc -p s3/tsconfig.json
node s3/build.mjs
node s3/browser.test.mjs <증거 디렉터리>
```

- 기존 component 50 PASS, 새 인증 API 21 PASS. 합성 HTTP 응답과 실제 일회성 ML-DSA 키를 사용했다.
- 같은 bearer·세 고정 route/본문·헤더 정책, 두 opt-in IO0, 잘못된 origin IO0, select/destroy/revoke/hold 중 직접 조회에서 서명0·방송0, 401/403/503·동시 철회·stale·2초 지연을 검증했다.
- 지연 세션·이전 401이 새 계정 인증을 덮어쓰거나 철회하지 않음, 지연 결과의 UNKNOWN 유지, 응답 유실/재로그인 후 추가 TX0을 검증했다.
- Chrome 합성 DOM 13 PASS. 기존 화면 검사를 새 authenticated factory로 배선하고 세 chain route의 bearer를 fixture에서 검사했다. 전 네트워크 차단 상태이며 서비스는 시작하지 않았다.
- typecheck/build 성공. sandbox Chrome 시작 제한은 별도 로그로 보존하고 허용된 재실행 결과를 구분한다.

실제 서비스 HTTP·B adapter·DEV12/4검증인 NOT_RUN, runtime pin 미발급, 지출 €0. 경제/UNKNOWN/protocol/lock/main 변경0. `G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false`와 원 부모 blocker 유지. 새 후보 CTO→Security 완료 후 L-R이 통합 build/manifest를 갱신한다.
