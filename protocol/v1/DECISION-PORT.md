# 공통 판정 포트 v1 — rc3

G-01~04 수정 계약 후보. CTO 결정, 2026-09-29. 실제 앱·WAL·원장 구현은 범위 밖이다. 이 문서와 JSON 벡터는 독립 Security→QA 검토 전이다.

## 함수 경계

세 언어는 `authenticate_order`, `evaluate_snapshot`, `admit_order` 결과를 별도 필드로 노출한다. 단일 `valid=true`/`OK`로 합치지 않는다.

- authenticate_order: canonical OrderV1 bytes, signature, expected chain/genesis/market/version, 확정 등록 account `{key_type, raw_key}` 또는 명시적 unregistered, snapshot id/height를 받는다. 등록 타입은 정확히 `ML-DSA-65`여야 한다. 동일 bytes라도 OTHER는 ACCOUNT_KEY_MISMATCH. 미등록은 ACCOUNT_KEY_UNREGISTERED. 타입 인자가 없는 adapter는 성공 대신 NOT_CONNECTED를 보고한다. 정상 타입이라도 길이·owner 바인딩·등록 bytes 일치·실제 서명을 모두 검사한다. 로그인 성공은 이 결과를 대체하지 않는다.
- evaluate_snapshot: 인증 성공에 이어 동일 snapshot id/height의 정책을 평가한다. snapshot은 `source=SYNTHETIC`를 명시하며 fee version/active bps, epoch, revoked, ID 상태, 누적 체결, 확정 가용 잔고를 공급한다. null/누락 상태를 false/0/무한 잔고로 채우지 않는다. 필요한 상태가 미연결이면 snapshot 결과 NOT_CONNECTED, 미실행이면 NOT_RUN. synthetic 정책 PASS는 인증 또는 원장 승인 증거가 아니다.
- admit_order: 실제 앱의 원자 예약·ID 처리·WAL/outbox 영속화와 실제 ACK 증거가 있어야 ACKED를 반환한다. 본 S0 포트에서는 `ack=NOT_CONNECTED`, `wal_replay=NOT_RUN`, `ledger=NOT_CONNECTED`를 고정한다. 정책 통과를 ACKED/PENDING/COMMITTED로 승격하지 않는다. 실제 ACK/재생 시험은 구현·독립 검증 범위에서 별도로 수행한다.

출력: `{authentication:{status,code}, snapshot_policy:{status,code,source,snapshot_id}, ack, wal_replay, ledger}`. status는 PASS/REJECTED/NOT_RUN/NOT_CONNECTED. PASS code=OK, 미실행·미연결 code=null. source=SYNTHETIC 또는 CONFIRMED; 본 벡터는 SYNTHETIC만 사용한다. snapshot 누락 시 snapshot_id=null. 인증 거절/미연결이면 정책 NOT_RUN. 정책 code를 인증 code로 덮어쓰지 않는다.

## 테스트 입력과 우선순위

`vectors/decision-port.json`의 authentication_result는 **실제 암호 실행 결과가 아니라 명시적으로 주입한 합성 전제**다. Python은 정책 및 결과 구조의 예만 검사한다. C/D/F adapter는 실제 제출 bytes/signature와 등록 key_type/raw_key를 최상위 인증 함수에 전달하고 실제 결과로 대체한다. 정상·다른 타입·미등록·다른 raw bytes·서명 변조를 다시 실행해야 한다. 변경 q/p/cap은 반드시 새로 서명하며 기존 서명의 필드만 바꾼 입력을 인증 PASS로 간주하지 않는다.

snapshot 필드: id, source, height, expiry_height, epoch_matches, revoked, id_state(NEW/CONFLICT), cumulative_ok, confirmed_balance_ok, q, p, active_bps, cap. 모든 숫자는 정규 십진 문자열, height/expiry/q/p는 U64, cap U32. 인증 단계부터 CONTRACT의 우선순위를 따른다. 합성 정책은 필수 연결/정수 범위 → ID_CONFLICT → EPOCH_MISMATCH → ORDER_REVOKED → EXPIRED → DEV q/p 한도 → BPS_RANGE → FEE_CAP → FEE_GE_RECEIVE → CUMULATIVE_QTY_EXCEEDED → INSUFFICIENT_CONFIRMED_BALANCE 순서다. ID 재시도 성공은 이 신규 주문 포트의 NEW/CONFLICT 모델에 포함하지 않으며 실제 영속 ID 결과 재생은 NOT_RUN.

합성 정책은 base=q*1000, quote=q*p 양쪽 수취액에 활성 수수료를 검사한다. q=p=1, active=cap=25에서 인증 PASS와 정책 FEE_GE_RECEIVE를 동시에 보고해야 한다. 이는 지정한 합성 체결 후보 정책이며 미래 모든 부분 체결의 허가가 아니다. 실제 fill마다 수수료/누적량/확정 잔고를 재검사해야 한다. 확정 가용 잔고에 잠정 수취액은 포함하지 않는다.

## 수수료·cap·오류

`fee(receive, active_bps)`: receive 정규 U128 검증 → active_bps 정규 0..10000 검증(BPS_RANGE) → bps=0이면 0 반환 → 양수이면 U256 중간값으로 ceil(receive*bps/10000) → fee>=receive이면 FEE_GE_RECEIVE. receive=0,bps>0도 거절한다. receive 범위 오류가 bps 오류보다 먼저다.

`max_fee_bps`는 wire U32의 **서명된 상한**이다. 10001/U32_MAX도 유효 cap이며 활성 비율이 아니다. 활성 비율은 별도로 0..10000이어야 하고 active<=cap, 초과 시 FEE_CAP. cap 초과 U32는 INTEGER_RANGE. active=10000은 범위상 유효하나 양수 수취액 fee=receive이므로 수수료 검사에서 거절된다. 큰 cap은 활성 비율 제한을 완화하지 않는다.

공통 API mapping: INTEGER_RANGE/BPS_RANGE/FEE_CAP/FEE_GE_RECEIVE/ACCOUNT_KEY_UNREGISTERED/ACCOUNT_KEY_MISMATCH는 동일 code를 유지하고 HTTP 400, retryable=false, state=REJECTED로 매핑한다. 이는 제출 전 정책 거절이며 REJECTED_FINAL(확정 TX 실패)과 다르다. height는 신뢰된 관측 높이의 십진 문자열, 없으면 null이다. NOT_CONNECTED/NOT_RUN은 시험 상태이며 사용자 주문 승인/거절 API로 변환하지 않는다. 상세 key/account bytes는 오류 응답에 노출하지 않는다.
