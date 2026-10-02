# S2 출금 local-action binding 체크포인트

Candidate::local_action은 canonical LocalAction JSON을 받아 현재 snapshot의 등록 owner를 확인하고 (genesis의 Candidate, owner, kind, request_id)로 원본 결과를 고정한다. 원 요청 canonical bytes의 SHA256을 request_hash로 사용하며 최초 epoch/seq와 원문을 보존한다. prepare는 미체결 예약만 취소하고 D/P를 유지한다. abort는 더 높은 fresh snapshot에서만 동결을 해제하며 취소 주문을 복원하지 않는다. 같은 ID 재시도는 현재 신선도/epoch/동결 상태보다 먼저 원본 결과를 반환한다. owner는 API가 인증한 세션에서 전달해야 한다.

입력은 81 bytes 이하의 canonical 내부 경계다. unknown/중복 key, null, 잘못된 길이/대문자 hash, 비정규 JSON을 거절한다. 향후 HTTP 계층은 key 중복을 거절한 뒤 canonical bytes로 변환해야 하며 공백/필드 순서가 자유로운 계약 전송 JSON을 이 함수에 그대로 전달해서는 안 된다. 본 구현은 공통 protocol 파일이나 의존성을 변경하지 않는다.

검증: 신규 4개 포함 시퀀서 30개 PASS(1.77초), all-targets clippy PASS. epoch 갱신 후 원본 결과/epoch 보존, owner·kind별 namespace, 동일 입력 재적용 결정성, 높은 snapshot 이후 abort, 취소 주문 미복원, 중복/추가 필드/비정규 입력 거절을 검사했다. 기존 signed/snapshot disk recovery 시험도 포함된다. 신규 local-action 자체는 아직 디스크에 기록하지 않는다.

재현(저장소 checkout 기준):
```sh
export RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo
cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer
cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings
```

한계: 출금 JournalRecord/CommandResult/receipt·semantic replay 연결, 전역 admission gate, 최대 정정 용량, 실제 서비스 API·프로세스 crash 통합이 남았다. 미영속 Outcome은 LOCAL_ACCEPTED 응답이 아니다. 원격 CI 미확인, 처리량/CPU/RSS 미측정, 유료 비용 없음. 실제 RPC/체인 인수는 D/F의 범위이며 제품 PASS 또는 CTO→Security 심사 요청이 아니다.
