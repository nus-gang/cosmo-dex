# S3 rc3 증거 저장·용량·의미 재실행 준비

2026-10-05 · [NUS-56](/NUS/issues/NUS-56) · **공개 서비스/내구 ACK 미완료**

[NUS-65](/NUS/issues/NUS-65)의 Security→QA 승인 head `51ab101ff5408b873cba101129b4c06962d56cc7`, tree `2a88bdf883434c52172ba59bc82017c27e962f5c`를 전용 branch에 병합했다. `protocol/s3/STORAGE.md`·schema·manifest·lock은 그 승인본 그대로다. `s3/1`, `s3/2`를 자동 변환하지 않고 새 `s3/3` namespace만 받는다. S1/S2 데이터나 HELD_S2 outbox를 가져오는 경로는 없다.

`S3-COMPONENTS.md`와 `S3-CANDIDATES.md`는 앞선 구현·실패 이력이다. 현재 상태와 제한은 이 문서 및 `evidence/s3-storage/REPORT.md`를 따른다.

## 제공하는 비공개 경로

- `evidence::Objects`: exact raw bytes의 SHA256/length/media type을 재검증한다. RPC 16MiB, TxRaw139264B, canonical typed evidence262144B를 넘거나 비정상 UTF-8/JSON/중복 key이면 거절한다. metadata 슬롯은 Attempt, ResolutionEvidence, 최신 ChainSnapshot 중 정확한 schema로 해소한다. state/result/snapshot에서 도달하는 전이 ref의 정확한 SHA순 집합, type 충돌·누락·중복·역참조를 검사한다. 이것은 입력 cache이며 durable write가 아니다.
- `Candidate`: rc3 refs를 사용해 기존 원장/FIFO/outbox·한 미확정 batch·ML-DSA 주문·in-flight 보류·COMMITTED/VOID 대사를 계산한다. `provide_evidence`는 원문을 private cache에 넣을 뿐 ACK를 만들지 않는다. 같은 ID retry도 증거를 다시 확인한다. terminal receipt는 대응 attempt를 INCLUDED_SUCCESS로 함께 갱신하고, 이미 확정 실패/불포함인 attempt와 모순되면 닫는다. 경제 효과와 D/P 해제는 이후 `apply` 후보에서만 계산된다.
- `capacity::certificate`: 승인 Python oracle의 checked-u128 이식이다. 전체 누적 이력, 최대 Unicode escape, N번 미래 정정·25N attempt metadata·181N+O+2 refs, Q=40N+O+2와 파일 allocation overhead를 포함한다. 반환 B는 **확보된 공간이 아닌 계산 상한**이다. Jmax 초과/overflow/미지 무한 schema는 거절한다. 이를 statvfs나 reserve 파일 크기에 대입해서 ACK를 발행하면 안 된다.
- `record::Prepared`: 전후 candidate에서 완전한 CommandResult·Correction audit·JournalRecord를 조립한다. CorrectionRecord prefix와 before/after hash·request hash·audit 1:1을 유지한다. `replay`는 저장 EngineState를 복원 상태로 신뢰하지 않고 원 서명/명령/같은 H snapshot/attempt/receipt를 재실행해 full state/result/record를 대조한다. 신규 fill ID, book admission/FIFO, C/R/D/P, applied batch, correction revision이 이 비교에 포함된다. caller는 `prepare` 결과를 공개하기 전에 의미 재실행·물리 예약·원자 journal commit을 연결해야 한다.

내부 기록의 `request_wire`/signature는 빈 bytes다. 서명 주문/취소와 인증된 LocalAction만 원 wire를 저장한다. CORRECTION request hash는 승인 도메인과 네 필드를 그대로 쓴다. 그 외 내부 명령의 request hash는 관측 snapshot ID이며 명령 종류/seq/원 입력은 full record에 결합된다. COMMITTED receipt를 보존하면서 attempt를 성공으로 판정하는 명령은 `RESOLVE_ATTEMPT`, 실패 증거/VOID receipt는 `VOID_BATCH`, 원장 대사는 `SETTLEMENT_APPLY` 또는 `CORRECTION`이다. 최종 실패 선택은 저장된 settle attempt 순서의 첫 INCLUDED_FAILURE이며 모든 settle attempt와 같은 H 조회를 함께 대조한다. 재생 때 동일 선택을 복원한다.

`latest_observation_ref`는 최신 ChainSnapshot 하나를 가리킨다. 과거 관측은 각 원 WAL snapshot에서 재실행해 history를 복원한다. 과거 관측 전체를 한 metadata 객체에 모아262144B를 넘기지 않는다. 현재 candidate는 최신 attempt 버전을 참조하고 과거 immutable 버전은 원 WAL의 ref 집합에 남는다. AppliedBatch.receipt_hash는 서비스 ResolutionReceipt canonical bytes의 일반 SHA256이며 compact chain receipt나 VIEW 도메인 해시와 구분한다.

## 실제 object IO와 미완료 경계

저수준 `journal::Journal`은 private0700 namespace의 `objects/sha256/<digest>`에 원문을 쓴다. file fsync→원문 재검증→hard-link no-replace publish→directory fsync를 수행한다. `<digest>.ref` descriptor도 같은 방식으로 length/type을 고정해 재시작 뒤 같은 digest를 다른 역할로 채택하지 않는다. reader는 O_NOFOLLOW·일반 파일·namespace 소유자/권한·상한·descriptor·exact bytes를 검사한다. metadata나 원문이 불완전하면 원문을 보존하고 닫는다. 이 descriptor 파일의 allocation/metadata도 최종 allocator가 A(x)의8192B overhead 안에서 입증해야 한다.

기존 `correction.reserve`16MiB 파일을 지우고 일반 공간으로 append하는 부분은 이전 storage fault 실험으로 남긴다. **이는 rc3 전용 예약 구현이 아니다.** `Journal::append`의 Commit은 해당 marker 증거이고, 공개 durable ACK를 허용하는 반환값이 아니다. S3 executable/REST/worker는 여기에 연결하지 않았다.

[NUS-66](/NUS/issues/NUS-66)은 CTO→Security가 저장 backend 지원 미입증 조사 보고서를 승인하여 완료했다. 64MiB APFS image에서 사전 할당8MiB의 같은 inode 소모는 일반 available458 blocks 조건에서 성공했으며, free=0·8192B metadata 상한·경쟁 writer·실제 B/B−1·reservation ledger crash는 NOT_RUN이다. 따라서 backend 지원 gate는 여전히 FAIL이다. Docker daemon 접속 실패도 조사 당시 관측이다.

후속 [NUS-67](/NUS/issues/NUS-67)에서 CTO가 전용 slot의 같은 inode 소모, metadata 상한, WAL/allocator commit 결합과 지원 gate를 고정하고 Security→QA 검토를 받는다. 설계 제안과 승인 원 plan을 initialPlan revision `a0b7f122-f34a-4c7f-9b10-897e7de7590f`로 전달했다. Exchange는 해당 exact 설계를 받아 물리 allocator·reservation ledger·단일 publisher·bootstrap/ACK 대사·전체 semantic crash matrix를 구현·시험한 뒤 원 업무의 CTO→Security 검토로 보낸다. 계산 B, sparse 파일, 일반 reserve 삭제를 지원 근거로 사용하지 않는다.

## 재현

Rust1.92.0 및 기존 Cargo.lock/cache, Python3 표준라이브러리를 사용한다. 새 의존성/lock 변경은 없다. `PAPERCLIP_RUN_SCRATCH_DIR`가 있으면 시험 파일은 그 아래에만 만든다.

```sh
cargo +1.92.0 test --manifest-path exchange/Cargo.toml --locked --offline --test s3_candidates --test s3_storage --test s3_journal --test s3_accounting --test s2_ledger --test s2_journal --test s2_sequencer --test s2_snapshot -- --nocapture --test-threads=1
cargo +1.92.0 clippy --manifest-path exchange/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
python3 -B protocol/s3/tools/check.py
```

`S3_CANDIDATE_EVIDENCE_DIR`는 재실행 trace, bootstrap, raw objects와 descriptor를 남긴다. source test key는 공개 fixture 전용이다. 실제 Chain receipt·SDK 합의·HTTP 동시성·D/F 통합·독립 심사는 NOT_RUN이다. 실제 runtime genesis=null; fixture의 genesis/context와 구분한다. 현재 결과로 S3 제품 완료·성능·내구 ACK를 주장하지 않는다.
