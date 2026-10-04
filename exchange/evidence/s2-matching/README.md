# S2-C matching 체크포인트

2026-10-02 Exchange. 전체 서비스 미완료이며 전문 검토/제품 PASS가 아니다.

`src/s2/matching.rs`는 sequencer의 live 주문과 인증·예약을 마친 taker 후보를 입력받아 결정적 MatchOutcome을 만든다. 가격 우선/FIFO, maker expiry, CANCEL_TAKER_REMAINDER, fill별 수수료 검사를 먼저 수행한다. 허용된 maker prefix를 실제 OrderBook-rs에 넣어 IOC로 실행하고 callback 전체를 예상 순서·ID·side·수량·가격과 대조한다. 성공 return의 fill과 callback은 일치 검사만 하며 합산하지 않는다. InsufficientLiquidity Err에서도 callback fills를 보존한다. 불일치/그 밖의 오류는 ADAPTER_RECOVERY_REQUIRED다.

OrderBook-rs `=0.13.1`, default-features=false pin/lock을 유지하며 dev dependency를 runtime dependency로 이동했다. 기존 tag 증거는 ../orderbook-tag.txt, tag commit a36218b9d2140e1c04ed22328f30fb4977adb109. 캐시된 배포 crate Cargo.toml/LICENSE도 확인했다. 새 버전/lock 변경 없음. 상위 라이브러리의 clock/UUID/engine_seq는 결과에 쓰지 않는다. library ID는 고유 admission_seq로 지정한다.

현재 구현은 매 명령마다 허용 maker들로 비공개 upstream book을 재구성한다. canonical live book과 GTC 잔량·취소·만료 적용은 sequencer가 소유한다. STP/fee reject 다음 maker를 건너뛰지 않도록 의도한 경계다. 성능 최적화/영속 upstream book 유지 구현이 아니며 처리량·CPU·RSS 미측정이다. 이 재구성 방식의 비용은 서비스 연결 후 측정한다.

검증: 신규 7 + 기존 ledger 5 + upstream IOC 2 = 14 tests PASS. matching 시험 표시 시간 0.00초(타이머 해상도 아래), ledger 0.05초; 성능 벤치마크가 아니다. all-targets clippy -D warnings PASS. Rust 1.92 도구 경로를 명시해 offline/locked 실행. macOS SDK 검색 경고는 tests.txt에 보존했다. 합성 잔고의 단위시험으로 실제 체인 예치 인수를 주장하지 않는다.

```sh
RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo test --manifest-path exchange/Cargo.toml --locked --offline --test s2_matching --test s2_ledger --test upstream_ioc
RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo clippy --manifest-path exchange/Cargo.toml --locked --offline --all-targets -- -D warnings
```

남은 구현: strict S2 schema/snapshot 신선도, 서명·ID binding·receipt, 전역 sequencer와 canonical fill ID, ledger/journal/outbox 원자 연결, connected-component epoch 정정/withdraw-freeze, 로컬 서비스 경계, 전체 replay/crash/capacity 시험. D/F 실제 체인·API 통합 인수와 CTO→Security 검토도 미완료다. 다음 heartbeat는 시퀀서 후보 상태와 snapshot/ID binding 연결을 진행한다.
