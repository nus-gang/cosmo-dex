# Wallet rc3 수정 인계

공통 계약 승인 SHA: 549ce150d6a9f21ec30f159d39a4d91c31dbd759.
계약 hash: a71a8c03fea5e4d2876612e821eafcb4b359a0b132157929b9924d6f8fecd73e.
벡터 집합 hash: 4851d9d674b2412ca8919d8347a71da13f9adf4426fe60b44e2a4a259f8bd948.
protocol 변경은 승인 커밋을 그대로 소비했다. 기존 bb50bb9와 evidence 루트의 rc2 결과를 보존하고 새 결과는 evidence/rc3에 둔다.

## 수정

- G-02: feeAtoms는 receive 정규 U128 검증 후 활성 bps 정규 0..10000을 BPS_RANGE로 검사한다. 0 비율은 0, 양수 ceil fee>=receive는 FEE_GE_RECEIVE. 공통 API 6개 오류 mapping은 HTTP 400 / retryable=false / REJECTED / height=null 또는 신뢰된 관측 높이를 유지한다.
- G-03: max_fee_bps는 서명된 U32 cap이다. 10001/U32_MAX를 cap으로 허용하고 활성 비율 제한은 별도로 검사한다. cap 20개 유효 wire 조합은 주문을 다시 서명·검증한다. U32 초과는 서명 전에 INTEGER_RANGE.
- G-04: authenticate는 bytes·도메인·키 등록·실제 ML-DSA 인증만 수행한다. 기존 verify는 인증에 epoch/expiry를 더한 호환 경계이고 validateDevOrder는 DEV 필드/fee cap만 검사한다. evaluateSnapshot은 합성 정책 전용이며 decideOrder가 실제 인증과 합성 정책을 연결한다. snapshot id/height, q/p/cap/expiry, epoch 판정을 서명 본문과 context에 결합한다. 필수 상태 누락·부정형·snapshot 불일치는 NOT_CONNECTED다.
- 등록 key type 누락은 NOT_CONNECTED, OTHER는 ACCOUNT_KEY_MISMATCH, 미등록은 ACCOUNT_KEY_UNREGISTERED. 같은 bytes라도 타입이 다르면 인증을 통과하지 않는다.
- 모든 판정에서 ack=NOT_CONNECTED, wal_replay=NOT_RUN, ledger=NOT_CONNECTED를 유지한다. 화면에도 이 경계를 표시한다.

## 재현 (checkout root)

    cd web
    npm ci --ignore-scripts
    npm run build
    npm test
    npm run test:browser
    node --experimental-strip-types scripts/decision-vectors.ts > evidence/rc3/decision-results.json
    python3 ../protocol/v1/tools/check.py

Chrome 경로는 CHROME_BIN으로 지정할 수 있다. 이번 로컬 검증은 기존 잠금 의존성 설치를 사용했다. fresh 설치는 CI 실행 결과와 별도로 구별한다.
Node와 실제 Chrome에서 같은 347 assertions를 수행한다. 34개 synthetic-spec 검사는 공통 fixture의 주입 인증 전제를 사용하는 정책 검증이다. 이를 실제 암호 검증으로 세지 않는다. 별도로 20개 cap 주문·tiny fill을 실제 재서명하고 등록 상태 5개와 서명 변조를 실행한다.
decision-results.json에는 공개 합성 벡터 21개와 실제 인증/정책 결과, 별도로 synthetic_spec_only 34개를 저장한다. 비밀키나 새로운 복구 seed는 출력하지 않는다. 공통 공개 KAT seed만 메모리에서 사용하고 키 버퍼를 지운다.

## 검토와 한계

CTO→Security 원 네이티브 리뷰로 재제출한다. G의 기존 420비교·7차이/FAIL과 수정 후 독립 재시험 NOT_RUN을 보존한다. 이 제출만으로 Go/Rust/TS 독립 교차검증 완료를 선언하지 않는다.
실제 REST/WS·체인 TX·영속 백업·서버 nonce 원자 소비·실제 ACK/WAL/원장은 연결하지 않았다. SYNTHETIC snapshot의 boolean들은 시험 입력이며 신뢰된 체인 상태로 승격하지 않는다.
Helix 보류·최소 자체 화면 결정과 기존 키 수명주기 ADR을 유지한다. 제품 출시·실자산·main 병합·후속 기능 승인은 포함하지 않는다.
