# NUS-38 시퀀서 후보 전이 체크포인트

2026-10-02 Exchange. 승인 계약 head `3571c3a`, 이전 구현 head `8b41880`에서 계속 구현했다. 실제 서비스 완료나 전문 검토 의뢰가 아니다.

- 검증 snapshot의 등록키/context를 소비하여 실제 OrderV1·CancelV1 ML-DSA-65 서명을 검증한다. 세션 owner는 별도 인자로 받고 본문 owner와 대조한다. 세션 발급/REST 서버는 아직 미구현이다.
- 인증 → ID binding 조회 → epoch/expiry → 정책/신선도/예약 → 매칭 후보 순서를 구현했다. 동일 본문의 randomized signature 재시도도 최초 결과/원본 증거를 보존한다. 다른 본문은 ID_CONFLICT, 결정적 거절도 후보 binding을 유지한다. genesis는 Candidate의 불변 snapshot에 묶여 있다.
- clone된 비공개 후보에서 전역 seq, 만료, C/R/D/P 예약, OrderBook adapter, FillIdentityV1 ID, 부분 체결/취소/IOC를 연결했다. 호출자는 이 후보를 영속화하기 전 공개해서는 안 된다. 현재 모듈은 LOCAL_ACCEPTED나 CommandReceipt를 생성하지 않는다.
- 신규 10개 시험 PASS(0.19초), all-targets clippy PASS. 실제 서명/변조·계정 권한·재시도·ID 충돌·거절 원자성·epoch/만료/신선도·부분 체결/취소·IOC callback·최악 D·P 사용 불가·직렬 과예약 방지·입력 재적용 결정성을 확인했다. 후보 두 개의 동일 입력 재적용은 디스크 재시작 증거가 아니다. 경쟁 프로세스 시험도 아니다.

## 재현

```sh
RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo test --manifest-path exchange/Cargo.toml --locked --offline --test s2_sequencer
RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo clippy --manifest-path exchange/Cargo.toml --locked --offline --all-targets -- -D warnings
```

합성 genesis/S2 서명 fixture 사용. 실제 예치/RPC/체인 인수는 이 시험에서 하지 않았다. 테스트 seed는 공개 fixture 전용이며 실자산 없음. 의존성/lock 변경 없음. macOS SDK 탐색 sandbox warning은 test.log에 보존했으며 시험과 clippy exit 0이다. 처리량·지연 분포·CPU·RSS 미측정, 추가 유료 비용 없음. 시험 시간을 개발 소요/납기 보장으로 환산하지 않는다.

## 남은 통합

snapshot 연속 갱신·epoch 연결 성분 정정·withdraw freeze, schema 정확한 result/state/receipt·outbox 직렬화와 오류 매핑, 최대 정정 용량 경계, journal commit 뒤 공개·재시작 재검산·crash, 서비스 및 인증 세션/조회 연결이 남았다. 내부 Outcome과 Order는 API schema 객체가 아니다. Candidate는 격리 계산 객체이며 단일 writer 실행기는 아직 연결 전이다. journal.rs의 기존 OS lock을 서비스 소유자가 보유해야 한다. S2 전체 또는 S2-AT/T01~16 PASS로 승격하지 않는다. CTO→Security 심사는 전체 구현/CI 증거를 갖춘 뒤 요청한다.
