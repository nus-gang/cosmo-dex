# S2 snapshot·정정 journal 연결 체크포인트

SnapshotRecord는 원 manifest Binding으로 새 snapshot을 다시 검증하고 이전 상태에서 연속 높이·epoch 정정을 계산한다. SNAPSHOT/CORRECTION record, 결과, 전체 상태/outbox, 외부 event ID를 하나의 journal commit에 저장한다. 정정 계획은 이전 상태와 새 snapshot에서 결정적으로 재구성되며 canonical JSON SHA256을 request_hash로 기록한다. 내부 snapshot 명령은 서명 제출 경로에서 거절된다. full snapshot ID를 external_event_ids에 기록하며 원시 체인 TX proof 인수를 주장하지 않는다.

복구는 서명 기록과 내부 snapshot 기록이 섞인 prefix를 순서대로 재실행하고 record 전체와 마지막 marker를 대조한다. 내부 기록에는 사용자 receipt를 만들지 않는다. 기존 성공 receipt는 그대로 보존한다. 중복 snapshot은 seq/디스크 효과가 없고 정정에는 journal의 별도 예약을 사용한다. marker 실패 시 후보 미공개, 후속 제출 차단, 재시작 fail-closed와 WAL 증거 보존을 확인했다.

검증: 시퀀서 26개(신규 2개) PASS, 1.66초. 실제 ML-DSA 시험 서명, 부분 체결, 직접 출금 모사 epoch 정정, 일반 후속 snapshot, 변조 5종·잘못된 이전 seq·중복·marker 실패를 포함한다. JournalRecord/CommandResult/Correction 3개를 고정 계약 schema oracle로 검사했다. all-targets clippy PASS.

재현:
```sh
export RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo
cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer
cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings
python3 exchange/tools/check_s2_projection.py exchange/evidence/s2-snapshot-record/snapshot-projections.json
```

아직 전역 admission gate·withdraw local-action binding/record·최대 정정 크기 계산·서비스 API·프로세스 crash 통합이 남았다. 현재 mode는 호출자가 공급하는 고정 투영 값이며 새 snapshot이 stale일 때 접수 재개를 결정하는 서비스가 아니다. MAX_PAYLOAD는 fixture ceiling이며 용량 증명이 아니다. 실제 RPC/체인 인수는 D/F에서 수행한다. macOS fsync는 로컬 PoC이고 full-fsync·장애 영역 복제 증거가 없다. 원격 CI와 처리량/CPU/RSS는 미확인/미측정이다. 제품 PASS나 전문 검토 요청이 아니다.
