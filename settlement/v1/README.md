# S0-E 정산 영수증·API 계약

NUS-14 · Settlement · 2026-09-29. S0 계약 어댑터와 합성 fixture이며 제품 정산기가 아니다.
기준선은 S0-A `889fda0c7181a696b4eb2a2649508c6192af8406` (`protocol/v1`, rc2).
공통 protocol 파일은 수정하지 않는다. NUS-5 M0의 상태/책임 경계를 유지하고 A의 명칭·정수 폭·영수증을 우선한다.

## 재현 및 소비

저장소 루트에서 Python 3 표준 라이브러리만 사용한다. 외부 패키지/키/네트워크 없음.

```sh
python3 settlement/v1/test_contract.py
python3 protocol/v1/tools/check-message-codec.py
python3 settlement/v1/mock.py
python3 settlement/v1/mock.py ws
```

`mock.py`는 HTTP 서버/WS 소켓 대신 URL→응답 및 JSONL 프레임을 제공한다. Wallet/QA는 `rest(api,url)`과 `EventConsumer.apply`를 직접 호출하거나 CLI 출력을 소비한다. 서버 배포는 하지 않는다.
`generate.py`는 A 필드에서 `api.schema.json`과 `fixtures.json`을 재생성한다. `evidence/`는 원시 시험 로그와 REST/WS 응답이다.
`manifest.json`은 A contract/vector/config hash와 모든 구현·fixture hash를 기록한다. 실행 SHA는 PR 및 이슈 제출 기록에 별도로 남겨 self-hash 순환을 피한다.

## REST/WS 의미

GET `/v1/markets/{market}/batches/{seq}?chain_id=...&genesis_hash=...`.
200 COMMITTED는 해당 키의 전체 BatchReceiptV1을 반환한다. 404 NOT_FOUND_AT_HEIGHT는 관측 높이에서 미발견이며 거절 증명이 아니다. 주입한 조회 장애는 LOOKUP_UNAVAILABLE, state=SUBMISSION_UNKNOWN이다. 공개 영수증 조회에는 주문/사용자 서명을 싣지 않는다.
영수증 키는 chain/genesis/market/seq이며 operator epoch 교체로 seq를 리셋하지 않는다. 과거 seq의 id와 hash를 모두 대조하고 최신 영수증으로 결과를 추정하지 않는다. 과거 영수증 누락은 RECEIPT_INCONSISTENCY, 신규 seq gap/이전 hash 불일치/중복 fill은 각 오류로 반환하며 새 이동을 만들지 않는다.

모든 정수는 정규 십진 문자열, U32/U64/U128 범위; hash는 lowercase hex32; 주소/키/서명은 canonical padded base64 및 A 길이를 따른다. JSON Schema의 `x-maximum`, `x-decoded-length`는 부가 규약이므로 **스키마 엔진 단독 검증은 불충분**하다. `strict_json` + `validate`로 중복/unknown key, 정수 범위, base64 길이/정규성을 함께 검사한다. 서명 바이트의 암호 검증은 Chain/Wallet 계약 구현 책임이다.

WS `/v1/stream`의 S0 최소 프레임은 entity_id, revision U64, observed_height U64, state이다. 같은 revision 동일 내용은 무효과, 역행 무시, 같은 revision 다른 내용은 REVISION_CONFLICT. COMMITTED/CORRECTED 종료 상태를 다른 상태로 바꾸지 않는다. PENDING→SUBMISSION_UNKNOWN→CORRECTED와 별도 COMMITTED fixture를 제공한다. 개인 스트림 인증·cursor/gap 복구는 NUS-5 설계를 유지하되 이 모의 프레임에서는 미구현이다.

## 안전 경계

- timeout/disconnect/미발견/조회 실패는 D/P를 해제하지 않는다. 조회 후 동일 batch ID·원본 바이트만 재시도한다. `Attempt`는 합성 불변 바이트 결합 시험이다.
- CORRECTED는 확정 실패, 모든 in-flight 시도 해소, 원명령 재생 완료가 모두 참일 때만 가능하다. 실제 증거를 검증하지 않는 입력 boolean은 신뢰된 상위 어댑터의 모의 포트다.
- COMMITTED 영수증은 정정 대상이 아니다. `release_D_P`는 확정 원장 적용과 예약 해제의 원자 연동 요구이며 여기서는 돈을 이동하지 않는다.
- A=C-R-D, P 제외. 음수 금지. 높이가 다르면 stale=true를 노출한다. 관측 시간 TTL·engine 높이·balance snapshot 및 stale 가용액 null은 후속 실제 조회 어댑터 연결 대상이다.
- retry는 신뢰된 BatchReceiptV1 형태의 요청 식별자를 받아 판정하는 내부 시험 포트다. 실제 제출 API로 노출하면 안 된다. Batch wire/context/current operator TX 권한·사용자 서명 검증이 먼저 필요하다.
- 실제 체인 commit, DB 원자성/장애 복구, SeenFill 영속성, Exchange WAL/ACK 재생·출금 경합·직접 회수·보존식은 NOT_RUN. 모의 시험의 성공을 제품 통합 성공으로 해석하지 않는다. 부분 실패 시험은 어댑터가 영수증/잔고를 쓰지 않음을 확인하며 실제 체인 rollback을 입증하지 않는다.

## 인수와 후속 연결

CTO 네이티브 검토 후 NUS-16(S0-G)에서 독립 검증 입력으로 사용한다. NUS-11(SRE)은 위 두 시험 명령을 공통 CI에 연결할 수 있다. 이 PR은 settlement 경로만 변경하며 CI/타 담당 파일을 덮어쓰지 않는다. main에 A가 아직 없으므로 A 브랜치를 base로 하는 stacked PR로 제출한다. A 머지 후 CTO가 base를 main으로 전환한다. GitHub workflow 실행은 이 PR에서 NOT_RUN이며 공통 CI 연결은 B 책임이다.
