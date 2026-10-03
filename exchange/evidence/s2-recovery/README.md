# S2-C signed journal 자동 복구 체크포인트

SignedRecovery::open은 검증된 bootstrap Candidate에서 전체 signed journal을 재실행하고 최종 marker까지 seq/hash/offset을 대조한다. 중간 상태는 공개하지 않으며 모든 record 검증 뒤에만 state·원본 receipt 인덱스를 제공한다. receipt 조회는 인증 세션 owner로 제한하며 writer lock은 객체 수명 동안 유지한다. 외부 전송 코드는 없다.

신규 부정 시험 1개와 기존 통합 시험 확장: 부분 체결·거절·취소 포함 4개 record 자동 복구, 결과/state hash/원본 receipt 일치, 타 owner receipt 비공개, 두 번째 writer 거절, 잘못된 bootstrap 및 의미 변조 record 거절, 원본 WAL/marker 무변경 및 증거 사본 확인. 시퀀서 21개 PASS (1.38초), all-targets clippy PASS. 원격 CI는 이번 변경에서 아직 확인하지 않았다. 처리량·CPU·RSS 미측정, 추가 유료 비용 없음.

재현(저장소 root):
```sh
export RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo
cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer
cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings
```
환경별 Rust/Cargo 설치 경로로 대체한다. 초기 기본 rustup home은 sandbox 쓰기 불가였고, 기존 1.92.0 설치를 지정한 뒤 실행했다. SDK 탐색 경고가 있었으나 시험 성공. 산술/계약/lock/genesis fixture 기준은 manifest 참조.

한계: signed ORDER/CANCEL prefix만 지원한다. SNAPSHOT/CORRECTION/withdraw 내부 record는 fail-closed하며 서비스 startup 완성으로 보지 않는다. caller는 마지막 embedded state가 아니라 신뢰된 최초 bootstrap snapshot과 mode를 제공해야 한다. 빈 journal에는 아직 bootstrap snapshot 저장/검증 경계가 없고 서비스 단계에서 연결해야 한다. 서비스 전역 gate·append 뒤 공개·withdraw ID·내부 record·최대 정정 용량·API/crash 통합이 남아 있다. LOCAL_ACCEPTED 서비스 인수·제품 PASS·CTO→Security 검토 요청이 아니다.
