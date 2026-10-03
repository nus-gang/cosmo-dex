# NUS-38 첫 구현 체크포인트 — 로컬 journal

2026-10-02 · Exchange · **진행 중, 전문 검토 요청 전**

NUS-36의 네이티브 Security → QA 승인 완료를 API에서 확인했다. 승인된 PR #26 head `3571c3a331a169a8eef231aad14f7816908dd2c5`에서 `exchange/nus-38-s2` 전용 worktree를 생성했다. 착수 시 원격 main은 `24029b811e5ec798bbe57f769de3d3f254c90ab7`이었다. 공통 protocol 및 다른 담당자 파일은 수정하지 않았다.

## 구현·검증

- `exchange/src/s2/journal.rs`: S2W1 72B header, payload/header SHA256, 16MiB payload 한도. 승인 WAL byte fixture와 일치한다.
- OS advisory exclusive writer lock을 전체 journal 생존 동안 유지한다. 별도 프로세스의 두 번째 writer를 차단한다. PID 파일 대체가 아니다.
- append → WAL fsync → marker temp fsync → rename → directory fsync 완료 후에만 Commit을 반환한다. 오류 후 handle은 poisoned 상태로 추가 쓰기를 거절한다.
- 완전/부분 UNKNOWN tail, 완료 frame 유실, header/payload/marker 손상, context 불일치를 fail-closed 처리한다. WAL 자동 truncate·삭제·빈 원장 성공 복구는 없다. 복구 오류 시 원본 evidence 사본을 fsync 보존한다. 증거 복사 자체가 실패해도 원본은 변경하지 않는다.
- 일반 append와 분리된 최대 정정용 reserve(16MiB + 72B header + 8192B marker 여유)를 실제 write/fsync로 확보한다. 신규 명령은 호출자가 제공한 최대 정정 직렬화 상한을 검사한다. CORRECTION만 reserve를 해제해 소비하고, 재확보 완료 전 신규 쓰기를 열지 않는다.
- 합성 S2 fixture의 실제 ML-DSA OrderV1 인증을 통과한 원문/서명을 journal에 보존하고 별도 프로세스 재시작 후 동일 bytes·서명·commit hash를 검증했다. 이는 합성 envelope 저장 시험이며 운영 주문 접수 영수증은 아니다.
- 신규 테스트 11개(하위 프로세스 helper 포함), 기존 Rust 회귀 18개 모두 PASS. 실제 upstream IOC Err 이후 callback 체결을 보존하는 기존 회귀도 PASS. 전체 clippy `-D warnings` PASS. 의존성/lock 변경 없음.

## 재현

```sh
cargo test --manifest-path exchange/Cargo.toml --locked --test s2_journal
cargo test --manifest-path exchange/Cargo.toml --locked
cargo clippy --manifest-path exchange/Cargo.toml --locked --all-targets -- -D warnings
```

Rust/Cargo 1.92.0, Darwin arm64, 기존 lock, 로컬 파일시스템. 임시 파일은 Paperclip run scratch를 사용하며 일반 실행은 OS temp를 사용한다. `tests.log`, `regression.log`, `clippy.log`, `manifest.json`에 원시 결과·입력 hash를 보존했다. 신규 시험 자체 실행시간 1.60초이며 개발 소요나 처리량 수치로 환산하지 않는다. CPU/RSS·ops/s·RPO/RTO는 미측정이다. 유료 자원·실자산·체인 연결 사용 없음.

## 아직 남은 구현과 한계

이 모듈은 저장 계층이다. JournalRecord/EngineState 전체 schema·의미 검증과 replay 재계산은 시퀀서의 책임이며 아직 연결되지 않았다. 테스트의 최소 record/context/outbox는 framing/원자성 시험용으로, 공통 schema를 통과한 제품 journal이라고 주장하지 않는다.

다음 Exchange 구현: 실제 단일 시퀀서와 서명/ID binding·receipt, C/R/D/P·FIFO/GTC/IOC·취소·만료·STP, upstream 어댑터, snapshot/cursor 신선도·epoch 연결 성분 정정, 보수적 최대 정정 인코딩 길이 생성기, 결과 hash 재계산·snapshot/WAL 인덱스, Rust 서비스/Settlement 연결 경계. 같은 이슈와 브랜치에서 계속한다. 기존 OrderBook-rs 0.13.1 pin/MIT 증거와 M0/S0 IOC 회귀를 재사용하되 런타임 어댑터 완료로 취급하지 않는다.

정정 reserve 파일 write·소비·재확보는 시험했지만 실제 ENOSPC·파일시스템별 물리 용량 보장·외부 reserve 훼손·최대 correction schema 직렬화 통합 시험은 미완료다. 프로세스 exit crash는 호스트 전원 손실과 다르다. macOS F_FULLFSYNC와 다른 장애 영역 복제는 검증하지 않았다. WAL과 marker를 함께 과거 상태로 되돌린 경우 로컬 파일만으로 탐지할 수 없다. 외부 ACK ledger 대조는 통합 crash harness에서 추가해야 한다.

S2-AT01~09 전체 PASS, T08/T10 분산 내구성 PASS, 제품 완료·출시·병합을 주장하지 않는다. 전체 C 구현·고정 head CI 준비 후 CTO → Security 네이티브 검토로 인계한다. 실제 체인 연결 인수는 D/F, main 통합은 I/CEO, 독립 main QA는 J 소유다.
