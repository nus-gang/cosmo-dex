# 공개 영수증 API 변경표 — 비활성 계약 후보

loopback `/dev-local/v1/`, 기존 Origin/session/ML-DSA 검증과 두 opt-in은 유지한다.
본 문서는 API 구현이 아니라 NUS-70 후속의 exact 규범이다.

| 경로 | 변경 전 | 승인·구현 후 |
|---|---|---|
| POST orders / cancels | 축약 command_result를 원 s3-dev-local/1로 표시하여 schema FAIL | 원 commit에서 만든 s3-dev-local-account/1, 원 source와 own account_result |
| POST receipts/orders / receipts/cancels | 같은 축약 응답 | 원 signed body를 검증한 **읽기 전용** 최초 결과 조회, 없으면404, 새 commit0 |
| POST withdraw/prepare / withdraw/abort | 원 local request를 접수하고 축약 응답 | 기존 요청/멱등키 유지, 새 공개 envelope. 동일 요청은 최초 결과를 반환 |
| GET receipts/commands/{seq} | 없음 | 새 **읽기 전용** 경로. U64 canonical path seq≥1; 유효 session의 본인 사용자 명령 또는 참여 내부 사건만 조회 |
| GET capabilities | envelope_version=s3-dev-local/1 | 이 기존 profile 표시를 보존하고 아래3필드를 추가하여 공개 receipt를 별도로 pin |
| GET account | 현재 계정 view | 별도 resource 그대로, 과거 receipt와 현재 수치를 합성하지 않음 |
| trusted C→D execute/query/receipt ledger | 원 s3-dev-local/1 + 전체 CommandResult | 변경0, 공개 계정 projection으로 대체 금지 |

capabilities의 추가 required 필드:

- public_receipt_version: `s3-dev-local-account/1`
- public_receipt_schema_sha256: 승인 schema.json **exact file bytes SHA256**
- trusted_receipt_version: `s3-dev-local/1`

client는 전체 Context와 위 public version/schema hash를 자기 승인 pin에 대조한다.
unknown/missing pin이면 신규 요청을 닫는다. 기존 envelope_version만 보고 새 receipt를
해석하지 않는다. profile guard의 envelope_version은 trusted 저장 의미로 불변이다.
capability나 query에 client가 principal/source hash를 쓰는 selector는 추가하지 않는다.

GET seq는 Context에 결합된 session만 허용하고 query string/body/추가 path suffix를
거절한다. 없는 명령, 타 사용자 명령, 무관한 내부 명령은 모두404 RECEIPT_NOT_FOUND다.
목록·전체 WAL 열거 API는 추가하지 않는다. 자신이 받은 seq 또는 자기 인증 view에서
얻은 사건 seq로 조회한다. auth가 없는 요청은 항상 먼저401이며 존재 여부를 노출하지 않는다.
서명 조회에서 valid 다른 session과 signed owner가 다르면 기존403 FORBIDDEN이다.

HTTP 오류 body는 정확히 code와 durable_ack=false 두 key다. 현재 요청의 모든 사전 검사를
마친 뒤 existing source 조회를 진행하고, storage 오류를 NOT_FOUND로 바꾸지 않는다.

| 상황 | HTTP / code | 효과 |
|---|---|---|
| 인증 없음·만료 session | 401 UNAUTHORIZED | source 정보0·commit0 |
| signed owner/session 불일치·비loopback·Origin 위반 | 기존401/403 auth 규칙 / FORBIDDEN | commit0 |
| 모호한 헤더·잘못된 signed wire·Context·서명 | 기존409 코드 보존 | receipt0·commit0 |
| 요청 cap 초과 | 413 RESOURCE_LIMIT | commit0 |
| 알 수 없는 공개 version/schema pin | client RECEIPT_SCHEMA | 송신·자동 fallback0 |
| 새 seq 경로 형식 오류 | 409 NON_CANONICAL_WIRE | query only, commit0 |
| 없음·타인·무관한 내부 명령 | 404 RECEIPT_NOT_FOUND | 상태 존재 구분0·commit0 |
| 원 source/해시/투영 원문 누락·불일치·초과 또는 IO/recovery gate | 503 RECOVERY_REQUIRED | 기존 WAL/원문/D/P 보존, 자동 복구·추가 효과0 |
| 원 commit에 기록된 정책 거절 | 200 / 새 receipt 안의 원 code·REJECTED | 원 효과를 그대로 기술; 추가 효과0 |
| 같은 client ledger 키의 bytes/source 불일치 | client CLIENT_RECEIPT_MISMATCH | 해당 session 자산 동작 닫기·양쪽 증거 보존 |

oracle의 RECEIPT_LIMIT/CANONICAL/SCHEMA/PRINCIPAL/CONTEXT 및 SOURCE_INVALID /
SOURCE_PROJECTION_MISMATCH는 검산 진단이다. 서버의 private 오류 상세를 그대로 내보내지 않는다.
실제 auth/signed/mutation 오류 우선순위는 상속 구현을 유지하며 source 오류는 일괄503이다.

성공 응답은 canonical JSON bytes이며 UTF-8 application/json, Content-Encoding identity다.
압축·newline·공백·숫자 변환으로 저장 bytes와 달라지지 않게 한다. receipt object bytes를
다른 일반 HTTP 헤더나 receipt 외부 wrapper와 혼동하지 않는다. client ledger에 session token을 저장하지 않는다.
