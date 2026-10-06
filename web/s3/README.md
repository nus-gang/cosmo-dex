# L-E 로컬 정산·출금 component 인계

[NUS-72](/NUS/issues/NUS-72). 입력은 L-D `f81c63fefaa89ee54ab3e2bee8ca5766221530e2`, tree `3c856e544d86bbbc5ce6a2a00ea62901bad64eff`이며 A/B/C ancestry를 포함하는 별도 clone이다. 기존 S0/S1/S2 파일·공통 protocol·dependency/lock을 변경하지 않았다. 새 설치·서비스 기동·main 병합·runtime pin 발급 없음.

## 실행 경계

`LocalClient(context, transport, chainPort, enabled=false, acknowledge=false)`는 기본 비활성이다. 두 opt-in 없이는 인증 요청도 실행하지 않는다. `mount(root, client, tabKeys, origin)`가 정산·출금 화면을 만든다. 운영자 키를 받지 않는다. LocalKey는 탭 메모리의 합성 사용자 키만 생성하고 직접 ML-DSA 서명한다. pagehide/destroy 때 키를 지운다. 영속 키 복구·키 내보내기는 없다. caller는 이 키의 공개키만 별도 새 S3 genesis 계정 등록에 사용한다. 공개 fixture seed를 실제 서비스에 쓰지 않는다.

L-R은 승인 context와 두 opt-in을 공급하고 아래 transport를 배선한다. 이 업무의 build는 실행 가능한 browser ES modules이며 독립 서버/launcher는 L-R 범위다. build manifest는 runtime 승인 pin이 아니다.

- REST transport: 정확히 `/dev-local/v1/` 상대 경로, 승인 loopback listener와 Origin, no-store, redirect error, 2초 deadline. 인증 challenge/session 뒤 capabilities를 검사한다. bearer는 private 탭 메모리다. owner는 로그인 challenge에만 제공하며 account/prepare는 서버 session owner를 사용한다. Origin 헤더는 브라우저가 만들며 client가 위조하지 않는다.
- ChainPort는 새 REST schema가 아닌 L-R **신뢰 로컬 Chain adapter의 TypeScript 배선 interface**다. `account(address)`는 동일 Context/확정 H의 account_number/sequence/owner_epoch/등록 공개키/gas, 원 관측 시각을 제공한다. stale 응답의 시각을 새로 찍으면 안 된다. client는 계정 공개키·height 일치와 2초 freshness, C 금액을 재검증한다.
- `broadcast(tx_bytes)`는 사용자 서명 원 TxRaw를 bounded loopback RPC에 한 번 전달한다. HTTP/CheckTx 성공도 UNKNOWN으로 유지한다. signer/private key를 adapter에 넘기지 않는다.
- `result(tx_hash)`는 B의 trusted RPC block+block_results에서 해당 TX index, 원 TxRaw, Context/chain/genesis/확정 높이와 code를 검증한 **확정 결과만** 반환한다. NOT_FOUND/CheckTx/mempool/timeout은 throw한다. 임의 `/tx` 성공을 확정으로 매핑하지 않는다. client도 원 TX bytes/hash·code/state·서명 당시 H보다 큰 확정 H를 확인한다. 결과 조회는 생성/방송을 하지 않는다.
- 직접 계정·방송·조회 HTTP route는 L-D가 제공하지 않는다. 기존 `/s1` 또는 `/s2` URL을 S3에 재사용하지 않았다. L-R에서 위 bridge를 실제 B adapter에 연결하고 기동 전 검토한다. ChainPort가 없으면 출금 버튼이 닫힌다. 실제 end-to-end API/브라우저·4검증인 DEV12는 L-T에서 검증한다.

## 화면과 안전 조건

C/R/D/P/A는 하나의 account revision으로 교체한다. atoms는 canonical 정수 문자열과 bigint, A=C−R−D이며 P를 더하지 않는다. 개발 보장 문자열·접수와 확정 구분·batch seq/id/hash/H/TX·잠정/불명/COMMITTED/정정을 표시한다. COMMITTED 표시는 projection의 batch receipt와 연결되며 client가 독립 light-client proof를 검증한다는 뜻은 아니다.

계정 전환은 generation을 증가시키고 view/session/capability/접수 안내를 즉시 비운다. 이전 generation 응답은 폐기한다. 동일 revision은 economic content 동일일 때 무효과, 충돌·역행은 닫힘, gap은 닫은 뒤 새로운 full account 조회에서 재동기화한다. 5초 원 관측 age, 2초 전달/query, monotonic 경과와 wall-clock 역행을 확인한다. 조회 시작 시 generation·단조 요청 번호·보류 barrier를 기록한다. 이미 처리한 요청보다 오래된 응답은 폐기하고, close 또는 recovery 응답은 barrier를 올려 당시 진행 중인 모든 조회를 무효화한다. 보류 후 시작한 새 권위 조회만 다시 열 수 있으며 같은 revision을 포함한 원 관측 시각 역행도 거절한다. 단절·recovery는 새 출금을 닫는다. 화면 timer는 상태 표시만 갱신하고 TX를 만들지 않는다.

출금 준비/명시적 해제는 승인 L-D endpoint만 호출한다. 개발 receipt는 ledger를 변경하지 않으며 다시 account를 읽는다. 준비 완료·fresh OPEN·R=D=P=0·불명/잠정 없음일 때만 직접 서명한다. 중복 클릭은 첫 await 전에 lock한다. 원 TxRaw/hash를 탭 history에 UNKNOWN으로 저장한 뒤 단 한 번 방송한다. 응답 유실/미발견에서는 조회만 허용하고 다른 TX 생성0. 확정 결과 이후에도 그 H 이상의 새 계정 projection을 받아야 다음 출금이 가능하다. 계정 전환 후에도 기존 키의 UNKNOWN latch를 유지한다.

## 검증·재현

기존 설치 dependency cache와 lock 사용. 새 설치 금지. checkout web에서:

```sh
node --experimental-strip-types --test s3/component.test.ts
node_modules/.bin/tsc -p s3/tsconfig.json
node s3/build.mjs
node s3/browser.test.mjs <증거 디렉터리>
```

`fixtures/ld-rest.json`은 L-D 직접 Rust handler의 원 합성 출력 복사본이다. Node 시험은 해당 shape와 합성 계정/시간 변형, 실제 일회성 ML-DSA 키·직접 TxRaw를 쓴다. Chrome 시험은 네트워크를 차단한 in-memory fixture와 실제 DOM으로 검증하며 HTTP/체인 서비스는 시작하지 않는다. `browser-fixture.ts`는 build entry에 포함되지 않는다. 모든 screenshot은 합성 component임을 표시한다.

`G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`, 표준 부모 blocker 유지. 실제 DEV01~14·runtime·main/CI·자산 보존 통합은 NOT_RUN. Helix 변경·새 UI 제품 채택이나 성능 보장을 포함하지 않는다. 독립 CTO→Security가 같은 최종 후보를 검토한 뒤에만 업무 완료다.
