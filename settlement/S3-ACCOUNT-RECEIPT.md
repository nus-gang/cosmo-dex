# 공개 계정 영수증 소비 경계 (NUS-71 후속)

Settlement REST는 승인된 공개 계약을 독립적으로 고정한다.

- 공개 응답은 `s3-dev-local-account/1`, schema SHA256
  `2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b`다.
  C의 compile-time version/schema가 이 값과 다르면 `Rest::new`가
  `RECEIPT_SCHEMA`로 닫히며 old/new fallback은 없다.
- trusted store/worker 영수증은 계속 `s3-dev-local/1`이다. Worker의
  prepare/broadcast/reconcile 입력은 `Command`, trusted recovery source와 원 TxRaw뿐이며
  `AccountReceipt`를 받는 API가 없다.
- 공개 성공 직전에 C의 immutable source를 다시 읽고 exact canonical bytes를
  `verify_account_receipt`로 대조한다. source/projection 불일치·IO·recovery는 공개
  `503 RECOVERY_REQUIRED`이며 cache나 축약 trusted result로 대체하지 않는다.
- `GET /dev-local/v1/receipts/commands/{seq}`는 인증 session을 먼저 확인하고,
  본인 명령/참여 사건만 반환한다. 없음·타인·무관 사건은 동일404, canonical seq 오류는409다.
  오류 body는 `code`, `durable_ack=false` 두 key만 가진다.
- 새 계약 aggregate/Context·genesis·빈 home·시험 키가 필요하다. 기존 home migration,
  guard 재발급, 원 trusted result/hash 변경은 지원하지 않는다.

이 component는 기본 비활성이고 실제 HTTP socket·4검증인·browser·runtime pin은
NUS-74 범위다. `G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false`를 유지한다.
