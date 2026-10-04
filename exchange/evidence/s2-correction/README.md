# S2-C epoch 정정·출금 준비 후보 전이

2026-10-02 Exchange, NUS-38. 승인 계약 head 3571c3a / 기존 시퀀서 bf2f6fa 위의 증분. 합성 snapshot과 실제 ML-DSA 서명 fixture 사용.

- 연속 snapshot 적용에 epoch 변경 owner의 pending-fill 연결 성분 정정 추가. fill은 생성 순서대로 역분개하고 과거 terminal 주문 및 상대방 후속 open 주문을 원 접수 순서대로 포함한다.
- 미체결 R만 취소하고 원 D/P/fee 기여분을 역분개한 뒤 새 C를 적용·대사한다. 원 receipt/원문/서명, lifetime filled_qty와 fill 기록 보존. 중복 snapshot 무효과, 영향 없는 주문 FIFO 보존.
- 로그인 owner 출금 준비는 동결→미체결 취소→D/P 검사. 보류 시 UNSETTLED_HOLD; D/P=0이면 OK지만 chain 권한 토큰이나 TX는 생성하지 않는다. 명시적 준비 취소는 더 높은 새 snapshot+신선도·장부 검증 뒤만 허용하며 주문을 부활시키지 않는다.
- 신규 6개+기존 10개 시험 PASS(0.18초), all-targets clippy PASS. 컴파일러 xcrun sandbox 환경 경고는 test.txt에 보존했다. 개발 시간과 테스트 시간을 동일시하지 않는다. 처리량/CPU/RSS 미측정, 유료 비용 없음.

재현 명령 (Rust 1.92.0, macOS 15.6.1 arm64):

```sh
cargo test --manifest-path exchange/Cargo.toml --locked --offline --test s2_sequencer
cargo clippy --manifest-path exchange/Cargo.toml --locked --offline --all-targets -- -D warnings
```

이 환경에서는 RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo 지정. 파일 hash 및 합성 genesis 관련 fixture는 manifest.json 참조.

한계/다음 작업: 아직 비공개 메모리 후보다. 전역 서비스 상태(CORRECTING/RECOVERY_REQUIRED 등), schema Correction reason/revision·result·receipt/outbox, journal commit 이후 공개, 최대 정정 직렬화 용량, 디스크 재시작/crash, 서비스 API 연결은 남았다. 실제 출금/RPC 통합·S2-AT05/06 PASS·LOCAL_ACCEPTED·제품 완료를 주장하지 않는다. 다음 heartbeat는 schema 영속 state와 journal 연결을 구현한다. CTO→Security 검토 요청 전 전체 기능·필수 시험·고정 head/CI 인계가 필요하다.
