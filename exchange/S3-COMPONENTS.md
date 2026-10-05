# S3 엔진 기초 모듈 — NUS-56 진행 산출물

후속 상태: NUS-64 rc2 승인을 소비한 비공개 통합 후보와 새 용량 blocker는 [S3-CANDIDATES.md](S3-CANDIDATES.md)를 따른다. 아래 내용은 최초 기초 모듈 산출 범위를 보존한다.

이 코드는 **S3 원장·방향성 의존 그래프·로컬 WAL 기초 모듈**이다. S3 정산 서비스, receipt 검증기, 원자 EngineState 공개 경로는 아직 제공하지 않는다. 합성 산술 fixture와 실제 파일시스템 시험을 제품/체인 정산 PASS로 해석하지 않는다.

## 기준선과 계약 차단점

- GitHub main: `bd9e473196ac86fdedf655b2c93e6931f54faa83`.
- 소비한 A 승인 head/tree: `0915375cac360f83d62a70587a0e7cf9c89604a1` / `ad98a57e149b861369f7d716aed8fd0325aef4f8`.
- A의 Security→QA 네이티브 검토 완료를 Paperclip에서 확인했다. `protocol/s3`와 모든 lock은 수정하지 않았다.
- `EngineState.corrections[] → Correction.after_state_hash → EngineState hash`의 순환 참조를 발견했다. `SCHEMA.md`는 `snapshot_id`의 제외 규칙만 명시하며 정정 후 상태 해시의 제외/투영/계산 순서는 정의하지 않는다. 임의의 zero placeholder 또는 필드 제외를 넣지 않았다.
- [NUS-64](/NUS/issues/NUS-64)의 CTO 계약 정정과 Security→QA 재심사 뒤 승인 SHA·manifest를 소비해야 전체 sequencer를 연결할 수 있다.

## 모듈과 호출 책임

`src/s3/ledger.rs`는 private candidate 회계다. S2의 atoms·fee 계산을 재사용하고 C/R/D/P를 검사한다. `reconcile`은 정확한 전체 owner 집합의 C, 확정/정정 fill 목록, 종료 잔량을 받아 clone에서 한 번에 재계산한다. 실패하면 기존 C/R/D/P 전체가 남는다. 이미 COMMITTED/CORRECTED인 fill의 반대 terminal 전이는 거절하고 원 fill·lifetime matched를 보존한다. 정정된 수량으로 예전 주문을 다시 체결하지 않는다. **이 API 자체에는 receipt/높이/자산 보존 proof 권한이 없다.** 완성될 sequencer가 권위 증거와 같은 높이를 검증한 후 호출하고 WAL을 commit해야 공개할 수 있다.

`src/s3/dependencies.rs`는 두 order와 두 `(owner,epoch,debit asset)` domain의 최신 pending 선행 fill을 최대4개 저장한다. predecessor는 원 command 순서다. `replay_append`는 계산한 목록과 저장 목록을 대조한다. 폐쇄는 앞으로만 전파되고 COMMITTED는 바뀌지 않는다. 같은 owner의 다른 확정 자산을 쓰는 독립 fill은 살아남는다. 전체 이력을 스캔하는 구현으로, 성능 수치나 지속 처리량을 주장하지 않는다. 외부 입력의 서명·order binding·정상 fill ID 검증은 상위 sequencer 책임이다.

`src/s3/journal.rs`는 승인된 S2 storage 구현을 S3 전용으로 분리한 것이다. `S3W1`, 72B header, 16MiB payload, OS 단일 writer, WAL fsync→marker fsync→rename→directory fsync→반환을 유지한다. S3 Context를 요구하고 다른 context의 home은 쓰기 전에 거절한다. S2 journal 파일과 HELD_S2 outbox의 import 경로가 없다. 큰 원시 증거는 SHA256 content-addressed object로 먼저 fsync하고 `evidence_refs`에 연결한다. append/replay는 길이·hash·존재를 재검증한다. partial/unknown tail을 잘라내지 않고 증거를 보존해 RECOVERY_REQUIRED로 닫는다. 상태 응답 revision 예약도 restart 뒤 증가하며 단조성을 잃으면 닫는다.

journal의 `maximum_correction_payload_bytes`는 상위 엔진이 계산한 실제 최악 직렬화 크기여야 한다. 이 모듈만으로 전체 correction capacity 산정을 완료한 것이 아니다. `JournalRecord`의 전체 의미 검증·EngineState hash·독립 ACK ledger 대사 역시 후속 연결 대상이다. 테스트의 저수준 journal record는 storage 시험용이며 완성 S3 서비스 schema fixture가 아니다.

## 이번 검증

- `s3_accounting`: 7개 시험. 0/25bps 2 BASE 매도·1 BASE 매수·잔량 취소, D_QUOTE=12→실제10 지출의 가격 개선2 해제, P 재사용 거절, terminal 효과1회/역전 거절, 잔여 R·미확정 D/P 보존, 최신 C 부족 시 원장 전체 유지.
- A `correction.json`: F1→F2→F3 정정, 같은 B의 별도 BASE 지출 F4 보존, F0 COMMITTED 보존. 그래프 fixture를 실제 Rust 모듈과 대조했다.
- A `correction-history.json`: 누적1000/1001 fills·200/201 orders·0/25bps 전체 목록·D/P·fee·lifetime 회귀. 이 fixture 자체는 합성 내부 이력이며 도달 가능한 서명 주문/HTTP 추적이라고 주장하지 않는다.
- `s3_journal`: 16개 항목(그중 subprocess helper1). 6 crash 경계 각각3회, 재생2회. before는0 commit, partial/WAL/marker는 증거 보존 후 recovery 거절, rename/commit 이후 프로세스 종료는1 commit을 확인했다. 전원 상실·OS cache 상실 시험은 아니다.
- S2 원장5개·journal14개 회귀와 전체 target clippy를 별도 확인한다. 정확한 최종 결과는 `evidence/s3-components/REPORT.md`와 원시 로그를 따른다.

실제 runtime genesis·높이·TX/batch ID·SDK receipt는 **NOT_RUN/null**이다. `F1` 등 짧은 ID는 기초 모듈 시험 이름이며 실제 protocol fill ID가 아니다. namespace 시험의 hex 값도 명시적 합성 context다. 기본0/별도25bps를 실제 같은 체인에서 전환한 적이 없다.

## 재현

기존 lock/cache와 Rust1.92.0에서 저장소 루트 기준:

```sh
cargo +1.92.0 test --manifest-path exchange/Cargo.toml --locked --offline --test s3_accounting --test s3_journal -- --nocapture --test-threads=1
cargo +1.92.0 test --manifest-path exchange/Cargo.toml --locked --offline --test s2_ledger --test s2_journal
cargo +1.92.0 clippy --manifest-path exchange/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
python3 protocol/s3/tools/check.py
```

시험 scratch는 `PAPERCLIP_RUN_SCRATCH_DIR`가 있으면 그 아래에 생성·삭제한다. 실제 세션은 주입 HOME의 rustup 쓰기가 거절되어 기존 read-only Rust toolchain과 run 전용 CARGO_HOME을 사용했다. Cargo.toml/lock 변경과 네트워크 설치는 없다.

## 이어서 구현할 승인 범위

Exchange가 NUS-64 승인 이후 맡는다: S3 서명 주문/sequencer 연결, 영속 FIFO outbox와 단일 미확정 batch, 원 BatchV2 bytes·seq/hash seal, 모든 attempt/원 실패 proof/VOID 검증, in-flight 관측 동결, COMMITTED+같은 H C+cursor+R/D/P+book/FIFO+revision의 원자 marker, 정정 결과·상태 해시, 최악 정정 공간 산정, 전체 semantic crash/replay 및 D 인계 API. 독립 survivor의 다음 유효 seq/hash 재구성도 이 연결에서 검증한다. 현재 단계에서 NUS-56 완료 또는 실제 정산 검증을 선언하지 않는다.
