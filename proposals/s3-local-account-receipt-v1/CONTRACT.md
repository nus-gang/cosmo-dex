# CTO-70-03 공개 계정 영수증 계약 후보

2026-10-08 · CTO · NUS-54 · Security→QA 심사 입력 / 비활성

`s3-dev-local-account/1`을 별도 공개 영수증 버전으로 제안한다. 승인 A
`fd9aa6ca9093817e4ab09d2ae835197a84bbade6`의 파일을 보존하고 이 디렉터리만 추가한다.
두 독립 심사 전에는 승인된 버전이 아니다. 본 후보의 승인도 제품 구현·runtime pin·기동 승인이 아니다.
G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false를 유지한다.

## 1. 결정과 적용 우선순위

원 `s3-dev-local/1.command_result`는 rc3 전체 CommandResult다. 다른 사용자의
ledger와 주문을 포함하므로 trusted 엔진·정산 어댑터·감사 ledger에서만 보존한다.
공개 응답은 본인 projection이며 `command_result`라는 이름을 쓰지 않는다.
required 필드에 빈 배열을 채워 원 영수증으로 위장하지 않는다.

승인 후 본 문서는 `proposals/s3-local-dev-v1/CONTRACT.md` §3 중 **공개 변경 응답과
공개 client receipt ledger의 형식**만 대체한다. 그 문서의 trusted receipt,
개발 저장 보장, 경제·proof·서명·WAL·Context 규칙은 그대로다. 원 rc3 schema와 hash,
표준 S3 API 및 원 F17/DEV09 해석을 변경하지 않는다. 이 overlay를 소비하지 않은
기존 후보의 축약 응답은 계속 CTO-70-03 FAIL이다.

## 2. exact 형식

`schema.json`은 모든 object의 required 전체와 additionalProperties=false를 명시한다.
공개 envelope의 정확한 9개 key는 envelope_version, profile_id, context, principal,
development_receipt, durable_ack, storage_assurance, source, account_result다.
schema의 required 목록이 기준이며 임의 확장 key는 거절한다.

고정값: envelope_version=`s3-dev-local-account/1`, profile_id=`s3-dev-local-v1`,
development_receipt=`LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE`, durable_ack=false,
storage_assurance=`UNPROVEN_HOST_SPACE`. Context는 전체 rc3 s3/3 Context다.
principal은 canonical padded base64의 정확히 20B owner다. public key·주소 별칭이 아니다.

source는 아래 네 값이며 전부 **같은 명령**의 저장 원문을 가리킨다.

| key | 권위 |
|---|---|
| command_seq | 원 CommandResult 및 JournalRecord의 U64 명령 순번, 1 이상 |
| record_hash | header72B를 포함한 원 S3D1 frame 전체 exact bytes의 SHA256 |
| command_result_hash | 원 글로벌 CommandResult의 NUS/S3/COMMAND_RESULT/V1 domain hash |
| after_state_hash | 그 명령 EngineState 전체의 NUS/S3/ENGINE_STATE/V1 domain hash |

domain hash는 rc3와 같은 `u32be(domain ASCII bytes length) || domain ||
u64be(canonical JSON bytes length) || JSON`의 SHA256이다. raw JSON SHA256과 다르다.
record_hash를 marker hash·최신 head hash·payload hash로 대체하지 않는다.
공개 envelope 자체에는 새 receipt ID·서명·자기 hash를 넣지 않는다. client는 exact
bytes와 SHA256(bytes)를 로컬 인덱스에 함께 보관할 수 있으며 인증 증명이라고 부르지 않는다.

account_result는 원 kind, request_hash, code, state, observed_height, snapshot_id를
그대로 복사하고, 아래 다섯 ID 배열과 ledger_changes를 §3에 따라 투영한다.
command_seq/after_state_hash는 source에만 있다. 글로벌 correction_results는 없다.
kind는 rc3 JournalRecord의 12개 명령, code는 schema에 고정한 현재 rc3 공개 결과 코드다.
미등록 코드는 임의 문자열로 내보내지 않고 source 불일치로 닫는다. 정책 확장은 재심사한다.

`state=LOCAL_ACCEPTED`는 code=OK일 때뿐이다. 그 외는 원 REJECTED를 보존한다.
**REJECTED가 경제 효과0이라는 뜻은 아니다.** 예를 들어 WITHDRAW_PREPARE의
UNSETTLED_HOLD는 주문 동결·잔량 취소를 기록하면서 출금 준비를 거절할 수 있다.
ledger_changes와 최신 readiness를 읽어야 하며 상태 이름을 새로 해석하지 않는다.
LOCAL_ACCEPTED도 COMMITTED·durable ACK·출금 가능을 뜻하지 않는다.

## 3. 원문 결합과 불변 projection

원 서버는 한 검증된 source 단위를 만든다. complete WAL prefix와 marker,
원 frame/원 result/state bytes·hash, 전체 evidence closure 및 rc3 결정적 semantic replay가
일치한 뒤에만 공개 projection을 계산한다. marker가 덮지 않는 frame의 성공 응답은 0이다.
reference.py는 이 단계의 hash/구조와 projection을 검사하는 정적 oracle이다.
이 oracle은 서명·auth·marker·경제 재생 검증을 대신하지 않는다.

principal은 유효한 기존 인증 session으로 결정한다. ORDER/CANCEL은 원 signed owner도
같아야 한다. WITHDRAW_PREPARE/ABORT는 기존 session 소유 local command이며 사용자
자산 이동 서명으로 승격하지 않는다. 본문 principal·계정 selector로 권한을 선택하지 않는다.

source 단위의 **after-state**에 남은 StoredOrder·OutboxFill·Binding tombstone을 쓴다.
rc3는 이 이력을 GC하지 않는다. 결과 이후의 상태·현재 owner epoch·최신 snapshot으로
과거 projection을 다시 만들지 않는다. 주문 hash/소유권이나 source 원문이 누락·중복·충돌하면
추정하지 않고 RECOVERY_REQUIRED다. 유효한 과거 terminal 결과를 현재 상태로 덮어쓰지 않는다.

1. O = source after-state에서 owner=principal인 모든 StoredOrder.view.order_hash.
2. F = 같은 after-state의 buyer_order_hash 또는 seller_order_hash가 O에 속한 fill ID.
   각 fill의 양쪽 주문 참조가 실제 원문에 있어야 한다. 자기거래도 ID를 한 번만 취급한다.
3. B = F에 속한 fill의 non-null batch.batch_id 집합.
4. affected_order_hashes는 원 결과 배열 ∩ O, created/corrected/committed_fill_ids는 각각
   원 결과 배열 ∩ F, applied_batch_ids는 원 결과 배열 ∩ B다. **원 배열 순서**를 유지한다.
   새로 정렬·현재 전체 ID 열거·페이지 절단·빈 배열 보충을 하지 않는다.
5. ledger_changes는 원 배열에서 owner=principal인 행만 복사한다. 0..2행, denom순,
   중복0, before/after denom 동일, 각 U128에서 A=C−R−D≥0, P는 A 계산에 넣지 않는다.
   상수0을 넣거나 fee/가격 개선/잔고를 projection에서 다시 계산하지 않는다.
6. 사용자 명령은 (kind, request_hash, first_command_seq)가 일치하는 원 Binding 하나의
   owner만 조회할 수 있다. 거래 상대가 같은 fill에 참여했어도 상대의 주문 **명령 영수증**은
   조회할 수 없다. 거절 주문도 원 Binding으로 식별한다.
7. 내부 명령(SNAPSHOT·EXPIRY·CORRECTION·정산 등)의 공개 조회는 위 ID/ledger 투영 중
   하나 이상이 비어 있지 않을 때만 허용한다. 원 글로벌 결과·정정 closure·상대 주문은 반환하지 않는다.

이 규칙은 부분 체결·복수 maker·누적 정정·정산 적용을 모두 같은 방식으로 다룬다.
정정 사건의 own corrected IDs와 own ledger 전후 값만 이 receipt에 담고, 상세 상태는
별도 인증 account 조회로 읽는다. 과거 주문 receipt를 이후 정정 상태로 수정하지 않는다.

금지: 타 owner/잔고/주문 hash·wire·signature, 운영자 키, raw TX/proof/evidence refs/path,
전체 EngineState·Correction. capability와 account 조회에도 기존 계정 격리를 유지한다.
허용된 source hash·global seq·own fill/batch ID·height는 공통 사건 상관관계와 활동 순서를
드러낼 수 있다. 본 계약은 타 계정 payload 격리이며 익명성·거래량 은닉 보장이 아니다.

## 4. 정규 bytes·정수·상한

rc3 canonical JSON: ASCII key 사전순, 공백/개행0, ensure_ascii의 소문자 escape,
중복 key0, 숫자 JSON literal0, surrogate 단독값0. null은 공개 schema에 없다.
hash는 소문자 hex64. U64≤18446744073709551615, atoms≤340282366920938463463374607431768211455.
선행0·부호·지수·소수·공백 숫자를 거절한다. TS는 BigInt 또는 decimal string을 사용한다.
원 signed wire·ML-DSA 바이트·0/25bps·ceil/cap의 경제 규칙은 변경0이다.

| 항목 | 고정 상한·근거 |
|---|---|
| 공개 canonical response body | 16,777,216B, rc3 WAL payload ceiling과 같음 |
| source frame | 16,777,216+72B; 원문을 재작성하지 않음 |
| 각 ID 배열 | 250,406개 = floor((16,777,216−1)/67), hex64 JSON 원소 최소67B에서 유도 |
| ledger_changes | 2행 = 본인의 DEVBASE/DEVQUOTE |
| 공개 JSON container depth | 8, root=1; 현재 schema의 최대6보다 큼 |
| 기존 signed request | 16,384B, auth120/300초·헤더/Origin 규칙 유지 |

원 result는 WAL의 base64 result_json 내부에 있으므로 canonical result bytes≤12,582,912B다.
projection은 원 부분집합이며 새 envelope/source/Context 오버헤드의 보수적 상한은4096B다.
따라서 유효 source에서 public≤12,587,008B<16,777,216B. 1000 fills/200 orders 같은
페이지 한도를 원 영수증에 적용하지 않는다. verify.py가 오버헤드와1001 IDs 및 배열 상한을 확인한다.
초과 source·예상 밖 projection은 자르지 않고 닫는다. pre-admission cap 계산을 구현하고,
이미 marker 완료 뒤 오류라면 원 결과/D/P를 보존하고 재조회·운영자 복구 경로를 남긴다.

## 5. 두 검증자의 의미와 재시작

공개 client ledger의 키는 (전체 Context, principal, envelope_version, source.command_seq).
요청 매핑은 (전체 Context, principal, kind, request_hash)→최초 source tuple이다.
동일 signed 요청·응답 유실 조회·같은 home 두 번 restart 결과는 source와 canonical body가
byte 동일해야 한다. 같은 키에 다른 source/body이면 CLIENT_RECEIPT_MISMATCH로 해당
session의 신규 자산 동작을 닫고 두 원문을 보존한다. 새 seq로 기록해 충돌을 숨기지 않는다.
과거 receipt 순번의 gap은 다른 계정·내부 명령 때문일 수 있으므로 client가 전체 연속 WAL을
가졌다고 판단하지 않는다. 현재 UI stream_seq gap 정책과 구분한다.

client는 schema·Context·principal·요청 연결·자기 bytes의 재조회 일치만 검증한다.
감춰진 글로벌 result/state/자산 보존 또는 서버의 정직성을 hash만으로 입증하지 않는다.
새 client의 첫 응답에 대한 독립 source 인증은 제공하지 않는다.

trusted ledger는 원 `s3-dev-local/1` 전체 bytes와 seq/record_hash/end_offset, raw WAL·
원 증거를 보존한다. 공개 ledger와 별도 타입/권한이다. trusted verifier는 marker/prefix/
semantic replay·원 result/state hash·principal Binding/참여와 **정확한 projection bytes**를
대조한다. 보안/QA는 trusted 증거에서 보존식·미확정 P 재사용0도 별도로 확인한다.
읽기·projection·cache 재구성은 새 경제 WAL 명령·재서명·새 receipt ID를 만들지 않는다.

과거 결과 조회는 유효한 현재 session과 같은 Context를 요구한다. 당시 주문 만료·owner epoch
변화·현재 snapshot stale은 이미 존재하는 과거 결과를 변경하지 않는다. 실제 auth 만료,
서명 위조, 다른 Context는 거절한다. 과거 조회 허용을 신규 주문·방송의 신선도 면제로 쓰지 않는다.
receipt 원문 검증이 불가능한 recovery 상태에서는 캐시만 믿어 성공하지 않는다.

## 6. 오류·API·호환성

구체 경로/HTTP 매핑은 API.md, 제품 시험은 acceptance.json을 따른다.
오류 응답은 정확히 {"code":STRING,"durable_ack":false}이고 source/원문/path를 싣지 않는다.
형식/인증/권한/제출 전 실패에는 영수증이 없다. 이미 기록된 원 정책 거절은 HTTP200과
새 envelope의 원 REJECTED 결과다. timeout·404는 명령 미접수/체인 실패의 증거가 아니다.

기존 축약 `s3-dev-local/1`을 새 버전으로 이름만 바꾸거나 재해시·자동 승격하지 않는다.
old/new public decoder의 자동 fallback0. 구 client는 capability pin 실패로 닫힌다.
trusted rc3 도메인·S3D1·guard.envelope_version=`s3-dev-local/1`은 불변이다.
공개 버전은 guard의 의미를 바꾸지 않고 새 manifest와 Context에 결합한다.

최종 runtime manifest는 기존 승인 rc3+A 파일 전부와 본 MANIFEST의 고정 파일을 포함한다.
MANIFEST 자체·genesis·키·실행 결과를 contract aggregate에서 제외하여 순환0을 유지한다.
본 후보 manifest는 runtime manifest가 아니다. component head/tree도 최종 manifest에 pin한다.
새 contract_hash가 Context·Chain init/query/restart·auth·주문/Batch 서명에 전파되어야 한다.
effective config bytes가 같아도 **새 Context·genesis·빈 home·새 시험 키**를 사용한다.
기존 home guard 재발급·result hash 변경·데이터 migration0. 같은 새 home의 restart는 허용한다.

fixture의 과거/합성 Context는 정적 검산용 식별자이며 새 runtime에 수락하는 allowlist가 아니다.
실제 새 genesis·ML-DSA·HTTP·restart·marker 장애·브라우저·4검증인 시험은 NOT_RUN이다.

## 7. 소비자·승인 경로

| 기존 업무 | 승인 exact A 이후 소유자 행동 | 검토 |
|---|---|---|
| C / NUS-70 Exchange | immutable 원 source 접근·projection·REST·원 trusted ledger 보존, P2 회귀·cap·재시작 | CTO→Security |
| D / NUS-71 Settlement | trusted result와 공개 응답 타입 분리, worker에 projection 입력 금지, capability·source verifier pin | CTO→Security |
| E / NUS-72 Wallet | 새 schema/bytes ledger·BigInt·Context/principal pin·오류/과거 조회·계정 전환 표시 | CTO→Security |
| L-R / NUS-73 SRE | 승인 C/D/E head ancestry·bytes, 새 contract aggregate/Context/genesis, final build/descriptor pin | 기존 CTO→Security |
| B / NUS-55 Chain | 기존 승인 dynamic Context init/query/restart 경로의 새 exact hash 연결 확인; 기능 변경 필요 시 원 업무 반환 | 변경 시 CTO→Security |
| L-T / NUS-74 QA | 새 exact runtime의 독립 실제 인수·client/trusted ledger 두 관점, fault 검출 | 기존 인수 경로 |

이 표는 책임 인계이며 타 업무의 이전 승인을 새 후보 승인으로 쓰지 않는다. 이번 run에서
새 업무·제품 구현·서비스·START/RPC·runtime pin·지출은 없다. A 승인 후 CEO가 기존
소비자 업무의 필요한 후속과 blocker를 조정한다. 원 NUS-68→67→56 gate는 유지한다.
Security와 QA는 같은 head/tree/MANIFEST를 새 단계 ID로 심사한다. 변경 요청은 CTO에게
반환하고 Security부터 재심사한다. 과거 완료 단계·판정·문서·증거는 보존한다.
