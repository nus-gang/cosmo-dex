# NUS-38 S2 snapshot 검증 체크포인트

2026-10-02 · Exchange · Draft PR #27 (`exchange/nus-38-s2`). 전체 서비스 완료나 전문 검토 의뢰가 아니다.

- 승인된 S2 snapshot fixture의 canonical hash를 Rust에서 그대로 검산한다. 중복/추가/누락 JSON key, 정수 범위, base64, 컨텍스트/시장 혼합, 계정/denom 순서, 공개키/주소 결합을 검사한다.
- 등록된 두 계정의 confirmed 합=module, bank 잔고 합+module=bank supply=고정 genesis supply를 checked u128로 대사한다.
- 비공개 후보 전이는 연속 높이만 허용한다. 동일 높이/동일 hash는 무효과, 동일 높이 충돌/역행/누락은 거절한다. 계정 번호/키 변경과 sequence/epoch/block time 역행, epoch 증가 없는 C 감소를 거절한다.
- epoch 증가 owner 목록은 후속 연결 성분 정정의 시작점으로 반환한다. 이 모듈만으로 원장 변경·정정·접수 재개를 수행하지 않는다.
- snapshot ID/cursor에 결합한 관측시각으로 5000ms 경계, 미래 block time 1000ms, RPC latency 2000ms를 검사한다. 신선한 조회라도 오래된 블록이면 STALE다. replay는 기록된 시각을 사용한다.

## 검증과 재현

macOS arm64, Rust/Cargo 1.92.0, 기존 Cargo.lock/의존성 유지. 합성 fixture 사용; 실제 예치·RPC·runtime genesis 증거가 아니다.

```sh
RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo test --manifest-path exchange/Cargo.toml --locked --offline --test s2_snapshot --test rc4
RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo clippy --manifest-path exchange/Cargo.toml --locked --offline --all-targets -- -D warnings
```

신규 9개 시험 0.02초, 기존 실제 서명/CLI rc4 1개 1.98초 PASS, clippy PASS. 실행 로그는 tests.txt/clippy.txt. 첫 빌드 SDK 탐색 경고 뒤 컴파일/시험 성공, 보존한 최종 실행에서는 경고 없음. 처리량·CPU·RSS는 미측정이며 시험 시간을 개발 소요나 성능 보장으로 환산하지 않는다. 유료 자원 사용 없음.

## 남은 연결과 제한

`Binding`은 trusted manifest/genesis에서만 구성해야 하며 클라이언트 snapshot endpoint를 제공하지 않는다. 실제 RPC 검증/출금 이벤트 근거는 D/F 통합과 sequencer에서 연결한다. 현재 모듈은 정합성 검증이며 Tendermint proof 검증이 아니다. genesis anchor→모든 높이 catch-up, 입력 실패 시 서비스 상태 동결, 관측의 WAL 기록, 연결 성분 정정·C 교체는 아직 미구현이다.

시퀀서/실제 Order·Cancel 인증/영속 ID binding/receipt·outbox·정정 용량 산정·프로세스 서비스·통합 crash 시험이 남았다. 다음 구현은 검증된 snapshot을 입력으로 받는 시퀀서 후보 상태 및 인증/ID binding 연결이다. T01~16 full PASS 0/16 경계와 로컬 fsync 한계는 유지한다.
