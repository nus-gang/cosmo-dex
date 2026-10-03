# S2 출금 journal·복구 체크포인트

WITHDRAW_PREPARE/WITHDRAW_ABORT를 기존 직렬 commit 경계에 연결했다. canonical LocalAction 원문·빈 signature와 SHA256·관측/시각·결과·전체 상태를 같은 journal frame에 저장한다. WAL/marker fsync 성공 뒤 상태와 원본 CommandReceipt를 공개한다. UNSETTLED_HOLD도 동결/잔량 취소 결과와 함께 보존하며 D/P는 유지한다. 같은 owner/kind/ID 재시도는 원 receipt를 반환하고 새 frame을 쓰지 않는다.

복구는 순서대로 서명 주문·snapshot/정정·local action을 재실행하고 결과/상태/record 전체 일치를 요구한다. local action owner는 해당 seq에 새로 생성된 유일한 binding에서 복원한다. 이는 ingress 세션 인증의 저장된 문맥이며 사용자 서명 증명이 아니다. 서비스가 반드시 인증한 owner만 전달해야 한다. journal hash는 파일 전체를 다시 작성할 수 있는 공격자에 대한 인증 수단이 아니다. SignedRecord라는 기존 타입 이름은 호환을 위해 보존했으며 로컬 세션 명령도 투영한다.

검증: 시퀀서 32개 PASS(신규 2개, 2.00초), all-targets clippy PASS. 실제 ML-DSA 주문→부분 체결→출금 보류→재시작/원 receipt→epoch 정정→abort→재시작을 검증했다. 잘못된 signature/request/state/result 및 추가 필드 거절, owner별 receipt 조회 제한, preflight 용량 거절의 무효과, WAL 이후 marker 실패의 후보 미공개/후속 차단/재기동 거절도 확인했다. 고정 계약 fixture oracle에서 5개 frame의 JournalRecord/CommandResult/EngineState 15개 schema 검증 PASS. oracle은 계약의 제한된 fixture validator이며 제품 API 검증기가 아니다.

재현:
```sh
export RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo
cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer
cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings
```

남은 범위: 전역 admission gate 및 관측 상태, 최대 정정 직렬화 용량의 보수적 계산, 실제 서비스 API/프로세스 crash 통합, 고정 head CI·CTO→Security 심사. 테스트 MAX_PAYLOAD 인수는 fixture ceiling으로 제품 용량 증명이 아니다. 실제 체인 연결 인수는 D/F에 남는다. 원격 CI 미확인, 처리량/CPU/RSS 미측정, 유료 비용 없음. 전체 제품 PASS·전문 검토 요청이 아니다.
