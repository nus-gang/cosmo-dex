# L-E 로컬 정산·출금 component 인계

[NUS-72](/NUS/issues/NUS-72). 새 입력은 공개 계정 영수증을 연결한 승인 L-D head `ccabc5ae15b4a5246a85d568a22910d3e17b4851`, tree `9f0b2cc6fe5bc296b73a7134c88e85207fdebafa`다. 공개 계약 head `ed4cf278cff78312ac606d6834901e0b8b265725`, schema SHA256 `2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b`를 별도 pin한다. 이전 Wallet 승인 후보의 5개 커밋을 이 exact D 후보 위에 이식했으며 기존 S0/S1/S2 파일·공통 protocol·dependency/lock은 변경하지 않았다. 새 설치·서비스 기동·main 병합·runtime pin 발급 없음.

## 실행 경계

`LocalClient.authenticated(context, transport, enabled=false, acknowledge=false)`가 L-R의 고정 Chain HTTP 경로와 같은 private 세션을 연결한다. 기존 constructor 주입 경로도 component 검증용으로 유지한다. 두 생성 경로 모두 기본 비활성이다. 두 opt-in 없이는 인증 요청도 실행하지 않는다. `mount(root, client, tabKeys, origin)`가 정산·출금 화면을 만든다. 운영자 키를 받지 않는다. LocalKey는 탭 메모리의 합성 사용자 키만 생성하고 직접 ML-DSA 서명한다. pagehide/destroy 때 키를 지운다. 영속 키 복구·키 내보내기는 없다. caller는 이 키의 공개키만 별도 새 S3 genesis 계정 등록에 사용한다. 공개 fixture seed를 실제 서비스에 쓰지 않는다.

L-R은 승인 context와 두 opt-in을 공급하고 아래 transport를 배선한다. 이 업무의 build는 실행 가능한 browser ES modules이며 독립 서버/launcher는 L-R 범위다. build manifest는 runtime 승인 pin이 아니다.

- REST transport: 정확히 `/dev-local/v1/` 상대 경로, 승인 loopback listener와 Origin, no-store, redirect error, 2초 deadline. 인증 challenge/session 뒤 capabilities를 검사한다. bearer는 private 탭 메모리다. owner는 로그인 challenge에만 제공하며 account/prepare는 서버 session owner를 사용한다. Origin 헤더는 브라우저가 만들며 client가 위조하지 않는다.
- ChainPort는 새 REST schema가 아닌 L-R **신뢰 로컬 Chain adapter의 TypeScript 배선 interface**다. `account(address)`는 동일 Context/확정 H의 account_number/sequence/owner_epoch/등록 공개키/gas, 원 관측 시각을 제공한다. stale 응답의 시각을 새로 찍으면 안 된다. client는 계정 공개키·height 일치와 2초 freshness, C 금액을 재검증한다.
- `broadcast(tx_bytes)`는 사용자 서명 원 TxRaw를 bounded loopback RPC에 한 번 전달한다. HTTP/CheckTx 성공도 UNKNOWN으로 유지한다. signer/private key를 adapter에 넘기지 않는다.
- `result(tx_hash)`는 B의 trusted RPC block+block_results에서 해당 TX index, 원 TxRaw, Context/chain/genesis/확정 높이와 code를 검증한 **확정 결과만** 반환한다. NOT_FOUND/CheckTx/mempool/timeout은 throw한다. 임의 `/tx` 성공을 확정으로 매핑하지 않는다. client도 원 TX bytes/hash·code/state·서명 당시 H보다 큰 확정 H를 확인한다. 결과 조회는 생성/방송을 하지 않는다.
- 직접 계정·방송·조회 HTTP route는 L-D가 제공하지 않는다. 기존 `/s1` 또는 `/s2` URL을 S3에 재사용하지 않았다. 새 authenticated factory가 L-R의 고정 chain/account GET, chain/broadcast POST, chain/result POST를 같은 private bearer로 호출한다. L-R에서 이 세 route를 실제 B adapter에 연결하고 기동 전 검토한다. ChainPort가 없으면 출금 버튼이 닫힌다. 실제 end-to-end API/브라우저·4검증인 DEV12는 L-T에서 검증한다.

## 공개 계정 영수증

capabilities는 trusted `s3-dev-local/1`과 공개 `s3-dev-local-account/1`을 서로 다른 필드로 고정한다. 공개 schema pin이 없거나 다르면 `RECEIPT_SCHEMA`로 닫고 old/new fallback을 하지 않는다. `withdraw/prepare`, `withdraw/abort`, `receipts/commands/{seq}` 성공 body는 최대 16 MiB의 exact canonical UTF-8 bytes로 읽는다. 중복 key, BOM/개행/공백, 숫자 JSON, 추가·누락 key, 비정규 U64/U128, 8단계 초과 nesting, 잘못된 Context/principal/hash/enum/ledger 식을 거절한다. atoms는 JS `Number`로 변환하지 않고 `bigint` 경계와 `A=C−R−D`를 검사한다.

tab-memory 공개 ledger 키는 전체 Context/principal/public version/command_seq다. 같은 키의 body 또는 source tuple이 달라지면 `CLIENT_RECEIPT_MISMATCH`로 현재 session의 자산 동작을 닫고 저장 bytes와 새 bytes를 모두 보존한다. token은 ledger에 넣지 않는다. 과거 command_seq 간극은 공개 receipt 이력 간극으로만 표시하며 현재 account revision gap과 합성하지 않는다. 계정 전환 후 지연 receipt는 ledger/UI에 반영하지 않는다.

`LOCAL_ACCEPTED`와 `REJECTED`는 원 code/state 및 본인 `ledger_changes` 수를 기록으로 보여 주지만 현재 account projection을 직접 변경하지 않는다. `LOCAL_ACCEPTED`는 chain `COMMITTED`, durable ACK, 출금 가능으로 승격되지 않는다. 출금 가능 여부는 계속 fresh current account의 C/R/D/P·fill/batch·withdraw gate만 결정한다.

## 화면과 안전 조건

C/R/D/P/A는 하나의 account revision으로 교체한다. atoms는 canonical 정수 문자열과 bigint, A=C−R−D이며 P를 더하지 않는다. 개발 보장 문자열·접수와 확정 구분·batch seq/id/hash/H/TX·잠정/불명/COMMITTED/정정을 표시한다. COMMITTED 표시는 projection의 batch receipt와 연결되며 client가 독립 light-client proof를 검증한다는 뜻은 아니다.

계정 전환은 generation을 증가시키고 view/session/capability/접수 안내를 즉시 비운다. 이전 generation 응답은 폐기한다. 동일 revision은 economic content 동일일 때 무효과, 충돌·역행은 닫힘, gap은 닫은 뒤 새로운 full account 조회에서 재동기화한다. 5초 원 관측 age, 2초 전달/query, monotonic 경과와 wall-clock 역행을 확인한다. 조회 시작 시 generation·단조 요청 번호·보류 barrier를 기록한다. 서비스 OPEN과 직접 출금 가능을 구분한다. 전체 검증을 통과한 fresh OPEN 중 withdraw_ready/withdraw_frozen, R=D=P=0, 미해소 batch/fill 없음까지 충족한 재개 응답만 요청 순서 필터를 적용한다. 이미 처리한 요청보다 오래되었거나 보류 전 시작했다면 폐기한다. 요청 시작 순서가 서버 관측 순서는 아니므로 닫힘 응답은 요청 번호·기존 barrier로 버리지 않고 Context/계정/원 관측 시각/원장 검증을 거친다. Context/계정/경제·revision/gap·관측 시각 검증은 요청 순서 필터보다 먼저 실행하며 오류는 보류한다. 출금 준비 해제·R/D/P·미해소 batch/fill 관측도 barrier를 올린다. 서비스가 열린 동안 출금 보류는 준비/해제 API 사용을 막지 않지만, 이미 닫힌 서비스는 오래된 요청으로 열지 않는다. close 또는 recovery 응답은 barrier를 올려 당시 진행 중인 모든 재개 조회를 무효화한다. 보류 후 시작한 새 권위 조회만 다시 열 수 있으며 같은 revision을 포함한 원 관측 시각 역행도 거절한다. 단절·recovery는 새 출금을 닫는다. 화면 timer는 상태 표시만 갱신하고 TX를 만들지 않는다.

출금 준비/명시적 해제는 승인 L-D endpoint만 호출한다. 개발 receipt는 ledger를 변경하지 않으며 다시 account를 읽는다. 준비 완료·fresh OPEN·R=D=P=0·불명/잠정 없음일 때만 직접 서명한다. 중복 클릭은 첫 await 전에 lock한다. 원 TxRaw/hash를 탭 history에 UNKNOWN으로 저장한 뒤 단 한 번 방송한다. 응답 유실/미발견에서는 조회만 허용하고 다른 TX 생성0. 확정 결과 이후에도 그 H 이상의 새 계정 projection을 받아야 다음 출금이 가능하다. 계정 전환 후에도 기존 키의 UNKNOWN latch를 유지한다.

## 검증·재현

기존 설치 dependency cache와 lock 사용. 새 설치 금지. checkout web에서:

```sh
node --experimental-strip-types --test s3/receipt.test.ts s3/component.test.ts s3/auth-api.test.ts
node_modules/.bin/tsc -p s3/tsconfig.json
node s3/build.mjs
node s3/browser.test.mjs <증거 디렉터리>
```

`fixtures/ld-rest.json`은 L-D 직접 Rust handler의 원 합성 출력 복사본이다. Node 시험은 해당 shape와 합성 계정/시간 변형, 실제 일회성 ML-DSA 키·직접 TxRaw를 쓴다. Chrome 시험은 네트워크를 차단한 in-memory fixture와 실제 DOM으로 검증하며 HTTP/체인 서비스는 시작하지 않는다. `browser-fixture.ts`는 build entry에 포함되지 않는다. 모든 screenshot은 합성 component임을 표시한다.

`G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`, 표준 부모 blocker 유지. 실제 DEV01~14·runtime·main/CI·자산 보존 통합은 NOT_RUN. Helix 변경·새 UI 제품 채택이나 성능 보장을 포함하지 않는다. 독립 CTO→Security가 같은 최종 후보를 검토한 뒤에만 업무 완료다.

## CTO-LE-R3-01 수정 검증

Node component 50 PASS, Chrome 합성 DOM 12 PASS. CTO 원문 재현 R1/R2 및 R3 준비 해제·D/P 두 사례를 수정 없이 실행하여 보류·서명0·방송0을 확인했다. R3는 이전 후보에서 두 사례 FAIL을 먼저 재현했다. 출금 제한 8종의 요청 순서 양쪽, 기존 barrier를 지난 보류, 요청 순서 필터 앞 revision/gap/원장/시각 오류, 서비스 OPEN 중 준비·해제 가능을 검증했다. 실제 네트워크·체인·DEV12는 NOT_RUN이며 독립 심사는 기존 CTO→Security로 재제출한다.

## ChainPort 인증 API 보완

상세 API·변경 검증은 [AUTH-API.md](./AUTH-API.md)를 따른다. 세션을 밖으로 꺼내거나 별도 저장하지 않는다. select/destroy/revokeSession 및 재로그인은 generation을 증가시키고 pending 요청을 abort한다. 응답 헤더와 JSON 완료 뒤 모두 generation/2초 상한을 확인하며 현재 세션의 401/403은 인증·capability를 철회한다. 이전 generation의 오류는 새 세션을 철회하지 않는다. UNKNOWN history는 세션 철회로 삭제되지 않으며 결과 조회도 현재 owner/generation에 묶인다.

이번 보완 검증: 기존 component 50 + 인증 API 21 = Node 71 PASS, Chrome 합성 DOM 13 PASS, typecheck/build PASS. 실제 인증 HTTP listener·B adapter·4검증인 DEV12는 L-T NOT_RUN이다. 기존 R3 승인과 별개로 새 exact 후보는 CTO→Security 재심사가 필요하다.
