# 개발 공개 계정 영수증 — C component 재심사 후보

승인 A `ed4cf278cff78312ac606d6834901e0b8b265725`의 [계약](../proposals/s3-local-account-receipt-v1/CONTRACT.md)·[API](../proposals/s3-local-account-receipt-v1/API.md)를 구현한다. 이 C 후보는 CTO→Security 재심사가 필요하며 runtime pin·서비스 기동 승인이 아니다. `G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false`를 유지한다.

**호환성 변경:** 개발 runtime의 계약 파일 집합에 승인 overlay MANIFEST의 `files_sha256` 60개를 추가해야 한다. overlay MANIFEST 자체는 빌드에서 고정하고 runtime 집계에서는 제외한다. 기존 rc3+A manifest 입력과 5 component descriptor 규칙은 보존한다. 이전 runtime 파일 집합을 수락하는 fallback이 없으며 새 Context/genesis/빈 home/시험 키가 필요하다. 기존 guard·receipt·WAL을 자동 이관하거나 다시 봉인하지 않는다. 시험 pin은 합성 fixture이고 최종 runtime 승인이 아니다.

## C의 읽기 API

| API | 의미 |
|---|---|
| `Engine::trusted_receipt_source(seq)` | 전체 prefix/marker·원 frame·참조 closure를 재검증하고 bootstrap부터 원 명령/서명을 semantic replay한 source. replay의 현재 state와 모든 trusted ledger entry를 공개된 revision에 대조한다. 없는 seq는 None, 저장/재생 오류는 RECOVERY_REQUIRED. 경로 입력·새 WAL·수리·callback 없음 |
| `ReceiptSource::frame/result/state` | 읽기 전용 원문 접근. frame은 디스크에서 읽은 exact S3D1 bytes. 전체 result/state는 trusted 전용이고 공개 응답으로 보내지 않는다 |
| `ReceiptSource::project(principal)` | source의 당시 after-state·Binding·주문/fill/batch 참여로 투영. 원 배열 순서와 값 보존. 다른 사용자 명령 또는 무관 내부 사건은 None |
| `Engine::account_receipt(seq, principal)` | source 검증과 projection 연결. principal은 인증된 어댑터에서 전달한다. 새 경제 명령·서명·receipt ID 없음 |
| `AccountReceipt::as_bytes/to_value` | exact canonical UTF-8 JSON 또는 그 사본. 외부 JSON으로 직접 생성하는 생성자 없음 |
| `Engine::verify_account_receipt(seq, principal, bytes)` | trusted source를 재검증하고 정확한 projection bytes 비교. source hash만으로 글로벌 자산/서버 정직성을 증명하지 않음 |

writer/semantic recovery/IO/poison gate는 공개 source 조회에도 적용한다. 캐시만으로 공개 성공하지 않는다. 기존 trusted `execute`, `query_signed`, `reconcile_receipt_ledger`의 `s3-dev-local/1` 전체 CommandResult 의미는 유지한다. trusted 과거 결과 조회와 공개 조회의 recovery 허용 범위를 혼동하지 않는다.

source에는 해당 명령의 command_seq·S3D1 full-frame record_hash·도메인 command_result_hash·after_state_hash가 연결된다. 최신 marker hash나 현재 상태로 과거 영수증을 갱신하지 않는다. source 조회는 전체 기록을 읽고 재실행하므로 비용이 기록량에 따라 증가하며 지연/처리량 보장은 없다. fault callback은 실제 execute에만 설치되며 source 읽기/재생은 F14 hook을 호출하지 않는다.

## 공개 REST component

`dev-local-settlement`의 기존 loopback·Origin·session·서명 검증을 유지한다. handler는 socket/listener를 시작하지 않는다.

- orders/cancels, signed receipts 조회, withdraw prepare/abort는 정확한 9-key `s3-dev-local-account/1`을 반환한다. `account_result`에는 본인 ID/ledger만 있고 `command_result`, correction 전체, 원 signed wire/proof는 없다.
- `GET /dev-local/v1/receipts/commands/{seq}`는 canonical U64≥1만 받는다. query string/body/추가 suffix는 거절한다. 인증을 먼저 검사하고 없음/타 사용자/무관 내부 사건은 동일 `404 RECEIPT_NOT_FOUND`다.
- capabilities는 기존 envelope_version을 보존하고 public_receipt_version, public_receipt_schema_sha256=`2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b`, trusted_receipt_version을 추가한다.
- wire 어댑터는 `Rest::handle_bytes`가 반환한 bytes를 application/json·Content-Encoding identity로 보낸다. 개행·압축·wrapper를 추가하지 않는다. 기존 `handle`은 in-process Value 호출자를 위해 남겨둔다.
- source 손상·누락·recovery는 503이며 body는 code/durable_ack 두 필드다. 정책 거절은 저장된 원 REJECTED·ledger 효과를 HTTP200으로 보존한다. UNSETTLED_HOLD는 효과0을 뜻하지 않는다.

새 접수의 private Prepared 단계에서 원 result cap·전체 참여자의 정확한 projection cap/schema를 검사한다. 16,777,216B 공개 body·250,406 ID 상한·2 ledger row 규칙을 지키며 페이지 절단하지 않는다. 예상 밖 source/projection 오류는 writer를 닫는다. 이미 완료된 marker 뒤 응답이 유실되면 원 결과를 보존하며 완전한 home의 명시적 재시작 뒤 동일 bytes를 조회한다. transaction/unknown tail은 자동 제거하지 않는다.

D는 원 trusted receipt를 worker 입력으로 사용하고 공개 타입을 별도로 전달해야 한다. E의 client ledger와 실제 HTTP/browser 검증, B의 새 Context 연결, SRE의 최종 manifest·5 descriptor·runtime pin은 각각 기존 담당 업무에서 인수한다. 이 component 시험은 실제 AR01~AR14·DEV01~DEV14 인수가 아니다.

## 변경 기록

- Added: 인증 seq 조회와 immutable source/projection·trusted 검증 API를 추가했다.
- Fixed: 공개 계정 projection에 원 글로벌 CommandResult 버전을 붙이던 CTO-70-03 결함을 승인 overlay로 교체했다.
- **Breaking:** overlay 없는 개발 runtime/home 입력은 거절하며 새 Context/genesis/home/모의 키로 시작해야 한다.
