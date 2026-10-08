# S3 개발 store·publisher 인계

이 구현은 NUS-70의 로컬 component 후보다. `G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`를 유지한다. 기본 build에 개발 경로가 없으며 `dev-local-demo` feature의 별도 `nus-s3-local-demo` binary만 입력 검증 및 오프라인 create/open을 제공한다. HTTP·RPC 방송·4검증인 기동·D/E/F 활성화·최종 runtime 승인·main/CI 인수는 포함하지 않는다.

## 입력과 신뢰 경계

`dev_local::Validated::new(Inputs)`는 다음 원문을 검증한다.

- 독립 심사 인계에서 받은 `approved_runtime_sha256`와 runtime manifest 원문. manifest에서 계산한 값을 승인으로 자가 발급하면 안 된다.
- A 후보 manifest `90169d322336a0c0de9bc6c48725d528d42fe74c78ea5b596fc7e059d747dda2`, rc3 manifest `3ff69e73057a2bb6dcff64820123d520b9ad3e5637abbd1ad7d38b8c1a49eb97` 및 상속 파일 전체. B의 `LocalDemoInputs`와 같은 5개 component descriptor 경로·집계 규칙을 사용한다. `COMPONENT_FIXTURE` scope는 공개 생성자가 거절한다.
- 승인 B와 동일한 runtime exact 9필드와 public MANIFEST/schema/version 세 pin. 공개 MANIFEST 원문 자체와 열거된 60파일을 runtime aggregate에 포함한다. 구형 6필드·원문 제외 fallback은 없으며 [공개 영수증 입력/API](S3-ACCOUNT-RECEIPT.md)의 새 Context·빈 home 규칙을 따른다.
- 정확한 fee0/25 effective profile, canonical guard의 8개 필드, 전체 Context, 새 genesis bytes 및 공개 사용자 roster. Chain의 InitChain/SDK 검증은 승인 B의 책임이다. C의 초기 snapshot은 인증된 로컬 chain adapter의 동일 높이 원문이어야 한다. 이 API는 light client가 아니다.
- 기존 rc3 `exchange/Cargo.toml` 원문은 시험용 `tests/support/rc3-exchange-Cargo.toml`로 보존한다. 현재 feature 설정을 상속 manifest에 다시 봉인하지 않는다. 실제 C 설정/lock/source는 component descriptor와 심사 증거에서 별도로 고정한다.

`Validated` 필드는 비공개이며 검증 후 바이트를 소유한다. 합성 descriptor와 test pin을 사용한 시험은 byte 검증 시험일 뿐, 실제 승인 runtime을 증명하지 않는다. 최종 runtime pin은 CEO/CTO의 승인 component 인계 후 SRE가 실제 binary에 대조해야 한다.

## 저장소와 장애 의미

`Engine::create(home, validated, bootstrap_bytes)`는 존재하지 않는 새 canonical 절대 경로만 받는다. 부모 경로는 준비돼 있어야 하며 suffix는 `.runtime/s3-dev-local-v1/<guard run_uuid>/fee0|fee25`다. 새 root0700와 단일 `writer.dev.lock`을 만든 뒤 guard file fsync → no-replace link/unlink → root fsync를 완료하고 store를 초기화한다. guard를 자동 재발급하지 않는다.

root 생성과 개방은 `/`부터 모든 조상을 `openat/O_NOFOLLOW`로 순회한다. 모든 하위 접근은 열린 디렉터리 FD에 상대적인 `openat/O_NOFOLLOW`, `mkdirat`, `linkat`, `renameat`, `unlinkat`을 사용한다. 디렉터리 열거도 `fdopendir/readdir`로 수행한다. owner·root0700·file0600·regular file·nlink1을 검사하고, root의 canonical path/device/inode와 열린 WAL/lock의 inode를 대조한다. 저장 root는 동일 사용자에 대한 보안 격리나 적대적인 호스트 관리자를 방어하는 경계가 아니다.

| 파일 | private 구현 의미 |
|---|---|
| `profile.guard.json` | 승인 계약의 불변 guard 원문 |
| `home.dev.json` | canonical path/device/inode, guard·bootstrap SHA256 결합 |
| `runtime.dev.json`, `genesis.dev.json`, `effective.dev.json` | 재시작에도 caller가 새로 검증한 입력과 바이트 대조 |
| `bootstrap.dev.json` | 최초 `ChainSnapshot` 원문. 잔고/book 덤프 import가 아님 |
| `journal.dev.wal` | `S3D1` 72B header, rc3 16MiB payload ceiling·체크섬·full frame hash |
| `commit.dev.json` | canonical `{command_seq,record_hash,end_offset}`. 모두 문자열이며 교체는 temp fsync/rename/root fsync 순서 |
| `objects/sha256/<hash>`와 `.ref` | 상속 raw/typed/TX 상한과 media·SHA·length를 매번 재검증. no-replace publish |
| `transaction.dev` 및 `*.tmp` | 작업 도중의 불확실성을 보존. 있으면 재시작 거절 |

표준 `S3W1`·표준 home은 상호 거절한다. 표준 Journal에도 개발 guard 거절을 추가했다. legacy `correction.reserve`, arena, free-space 기반 지원 판단은 사용하지 않는다. capacity certificate는 논리 상한 검사에만 사용하며 확보한 물리 credit으로 표현하지 않는다.

오류는 해당 writer를 `RECOVERY_REQUIRED`로 닫는다. 신규 명령·자동 정정·출금 준비·effect callback을 거절한다. 기존 chain 확정 자산을 되돌리지 않는다. 미상 tail·부분 원문·재해시 semantic 변조·임시 파일·inode 변경은 자동 truncate/delete/reseed 없이 원본 그대로 남긴다. 이 구현에는 손상 home을 수리하거나 미해결 `transaction.dev`를 삭제하는 API가 없다. 운영자 근거 검토와 체인 대사 없이 파일을 지워 재개하면 안 된다.

저장 오류의 writer 폐쇄와 영속 candidate의 `RECOVERY_REQUIRED`는 `Writer::ensure_open`에서 함께 검사한다. `UNEXPECTED_FINAL_REJECTION`을 commit한 직후와 같은 home 재생 후 모두 `execute`의 모든 명령·`committed_attempt`·`with_committed_attempt`가 `Error::Recovery("RECOVERY_REQUIRED")`로 끝나며 새 WAL/marker/state/result를 만들지 않는다. D가 reader 문자열을 먼저 확인하는 것에 의존하지 않는다. `CATCHING_UP`의 기존 receipt/확정 결과 대사는 계속 가능하다. `reader`, `query_signed`, `reconcile_receipt_ledger`는 읽기 전용으로 남고, 복구 중 동일 signed 요청의 원 결과는 `execute` 대신 `query_signed`로 조회한다.

완료된 marker와 완전한 prefix만 남은 crash/응답 유실은 명시적인 `Engine::open`에서 재생한다. `Prepared::replay`가 원 명령/서명을 다시 실행하고 full state·result·record를 비교한다. snapshot의 잔고나 FIFO를 그대로 역직렬화해 복원하지 않는다. guard/bootstrap 원문, seq/hash, fill, C/R/D/P, book/FIFO, cursor, correction revision, 결과 인덱스를 복원한다.

## D에 전달하는 Rust API

| API | 사용 조건과 결과 |
|---|---|
| `Engine::create`, `Engine::open` | 검증된 입력과 전용 home. 기존 home create·새 bootstrap으로 open 불가 |
| `execute(Command, raw_evidence, observation, now)` | 인증/신뢰 RPC를 마친 D 전용. 원문 저장/재검증 → semantic replay → WAL fsync → marker fsync/rename/dir fsync → 단일 Arc 공개 후 개발 envelope 반환. 무효과 반복 관측은 `None` |
| `Command::Signed` | ORDER/CANCEL 원 wire·서명과 인증 세션 owner. owner를 요청 본문에서 복사하지 않음. 동일 ID 재시도는 최초 결과, ID 충돌은 거절 |
| `Command::Local` | 인증된 WITHDRAW_PREPARE/ABORT. 직접 출금 서명/TX 실행은 제공하지 않음 |
| `Snapshot/Seal/Attempt/Resolve/Receipt/RejectFinal/Apply` | 기존 rc3 상태 전이·proof/capacity 검사 그대로. 경제 검증을 생략하는 별도 모드 없음 |
| `reader().get()` | 내부 전체 상태와 result map·commit·gate를 한 immutable Arc revision으로 읽음. 모든 계정/서명 증거를 담으므로 공개 REST에 그대로 노출하지 않음 |
| `query_signed` | 인증/서명을 다시 확인하는 읽기 전용 원 결과 조회. 신규 binding을 만들지 않음. 복구 gate에서도 이미 공개된 결과 조회 가능 |
| `reconcile_receipt_ledger` | client의 독립 receipt/seq/hash/offset entry 전체를 복원 결과와 비교. 응답하지 못한 commit의 추가 존재는 허용, client receipt 누락/변조는 거절 |
| `committed_attempt` | 열린 writer에서 commit된 attempt 조회. 저장 오류 또는 영속 RECOVERY_REQUIRED이면 거절. 반환 뒤 방송을 허가하는 token이 아님 |
| `with_committed_attempt` | 단일 writer lock 아래 store와 원 TxRaw를 재검증하고 PREPARED/UNKNOWN에만 D callback을 실행. callback은 Engine에 재진입하지 않고 bounded IO만 수행. D가 방송 intent를 먼저 저장하고 결과불명·재시도를 기존 계약대로 대사해야 함 |
| `trusted_recovery_history(&Commit, Option<u64>, usize)` | 같은 commit의 bootstrap부터 마지막 저장 관측까지 높이 순서로 최대 64개 원문/검증된 Snapshot을 반환. 적용된 C와 마지막 관측을 별도 anchor로 포함. 다음 페이지는 `next_height` 사용 |
| `trusted_recovery_attempt(&Commit, &str)` | commit된 TX hash로 PREPARED/UNKNOWN 및 terminal attempt와 그 참조 집합의 원 TxRaw·확정/부재 증거 조회. 없는 TX는 `None`. 방송 callback을 실행하지 않음 |
| `trusted_recovery_attempt_at(&Commit, usize)` | 같은 View의 `state["attempt_refs"]` 순번으로 최대 한 attempt와 증거를 읽음. 저장한 TX hash나 sidecar 없이 재시작 탐색 가능. 범위 밖 순번은 `None` |
| `trusted_recovery_failure(&Commit, &str)` | 같은 View의 batch ID로 저장된 `ResolutionEvidence` typed 값·원문·참조·정확한 도달 `Objects`를 반환. 실패 판정이 없는 batch 또는 없는 ID는 `None`. 원문이 빠졌으면 오류로 닫힘 |

### Trusted runtime 재시작 조회

NUS-73의 Snapshot 저장→Apply 전 및 terminal attempt 저장→receipt 수집 전 재시작 연결용 Rust API다. `dev-local-demo` 안의 `Engine`을 가진 신뢰 어댑터만 사용한다. 공개 transport schema·REST/browser·CLI route는 없으며 반환 타입은 `Serialize`를 구현하지 않는다. Rust caller의 인증과 Engine 소유권은 연결 어댑터의 책임이다.

먼저 `reader().get()`의 `commit`을 고정한다. 모든 history 페이지와 attempt 조회에 그 commit의 **command_seq·record_hash·end_offset 전체**를 전달한다. 각 조회는 단일 writer lock 안에서 store identity·guard·marker·object descriptor/SHA/length/media와 candidate/공개 revision 일치를 확인하고, 반환 전 store를 다시 검사한다. 중간에 새 commit이 생기면 `Invalid("STALE_COMMIT")`가 반환된다. 이때 부분 수집 결과를 버리고 새 View로 처음부터 수집한다. 존재하지 않는 과거 commit의 조회나 자동 snapshot 재작성은 없다.

```rust,ignore
let view = engine.reader().get()?;
let page = engine.trusted_recovery_history(&view.commit, None, 64)?;
// page.applied: 엔진 C가 적용된 Snapshot, page.latest: 마지막 저장 관측
// page.observations: raw 원문 + Context/해시/연속성 검증된 Snapshot
// next_height가 Some이면 같은 view.commit으로 이어서 조회
// 처음 재시작하면 state["attempt_refs"] 순번을 사용해 TX hash를 알아낸다.
let terminal = engine.trusted_recovery_attempt_at(&view.commit, 0)?;
if let Some(read) = terminal {
    let tx = read.evidence.resolve(
        &read.attempt["raw_tx_ref"], nus_exchange_contract::s3::evidence::TX,
    )?;
    // receipt/absence collector의 읽기 입력. 방송 허가로 사용하지 않음.
}
```

history의 `None` 시작점은 bootstrap 높이이고 지정 높이는 포함된다. limit은 1~64이며 초과/0은 `RECOVERY_PAGE_LIMIT`, 저장 범위 밖 높이는 `RECOVERY_HEIGHT_RANGE`다. 각 page는 최대 64개 observation 및 applied/latest 두 anchor를 반환하며 모두 기존 262,144B snapshot 상한을 따른다. bootstrap raw는 원 파일 바이트, 이후 observation raw는 원 저장 typed object다. 이전 observation anchor를 검증해 페이지 사이에도 기존 `Snapshot::advance`의 Context·height·hash·계정/epoch 연속성 규칙을 유지한다. Query 범위 오류는 commit이나 gate를 변경하지 않는다.

attempt 조회 입력은 기존 64자리 소문자 TX hash 또는 같은 commit의 attempt 순번이다. 임의 경로·EvidenceRef·object hash로 파일을 읽는 API는 없다. 현재 commit의 Attempt에서 도달하는 정확한 참조 집합만 store에서 읽고 typed Attempt와 원 TxRaw hash를 대조한다. 반환 `Objects`는 이 집합만 담은 메모리 사본이다. 기존 Attempt schema의 구조상 최대 20개 object(typed 1 + TX 1 + inclusion RPC 2 + absence RPC 16) 이내이며 각 media의 기존 byte 상한을 유지한다. 실제 semantic 유효 상태에서는 inclusion과 absence가 공존하지 않는다. 전체 store의 기존 검사는 계속 수행하므로 이 페이지 상한은 처리 지연/처리량 보장이 아니다.

`CATCHING_UP`에서는 미적용 latest/history를 조회할 수 있다. 저장 오류·`RECOVERY_REQUIRED`·poisoned writer에서는 모든 trusted 복구 조회 API도 닫힌다. 저장/참조 불일치 감지는 reader gate를 `RECOVERY_REQUIRED`로 바꾸며 새로운 WAL·receipt·경제 상태를 만들지 않는다. 기존 `query_signed`의 읽기 전용 원 결과 복원은 유지한다. terminal 데이터를 조회한 뒤에도 `with_committed_attempt`는 `ATTEMPT_TERMINAL`을 반환하고 callback을 실행하지 않는다. 재방송은 언제나 현재 writer gate와 기존 effect API 규칙으로 별도 판정해야 한다.

### 확정 실패 원문 복구

`trusted_recovery_failure(&view.commit, batch_id)`는 `RecoveryFailure { commit, resolution_evidence_ref, resolution_evidence, raw, evidence }`의 사본을 반환한다. `raw`는 저장된 canonical `ResolutionEvidence` 파일의 바이트이고, `evidence.resolve(&resolution_evidence_ref, TYPED)`와 일치한다. `resolution_evidence`는 이 바이트를 기존 schema로 검증·해석한 값이다. `Objects`에는 root와 그 원문에서 도달하는 TxRaw·block/results 원문만 들어 있다. 다른 batch, 최신 관측, 무관한 Attempt object를 합치지 않는다. 이 private Rust 타입에도 `Serialize`, REST, CLI route 또는 callback은 없다.

선택자는 같은 View의 `state["batches"][i]["batch"]["batch_id"]`인 64자리 소문자 hash다. 입력 경로·임의 `EvidenceRef`는 받지 않는다. 원래 실패 판정은 candidate의 batch별 저장값으로 선택하고, content address로 store에서 읽은 원문과 Context·전체 BatchIdentity·참조 집합을 대조한다. 최신 관측으로 실패 원문을 다시 만들거나 재선택하지 않는다. later Snapshot, CLOSE, VOID receipt, CORRECTION 뒤에도 그 배치의 원문은 동일하다. 정상 `CATCHING_UP`에서 읽을 수 있으며 `RECOVERY_REQUIRED`, 저장 오류, poisoned writer에서는 거절한다. 잘못된 선택자/없는 증거/`STALE_COMMIT`은 상태를 변경하지 않는다. `None`은 CLOSE/VOID나 보류 해제의 허가가 아니다.

기존 schema의 최대 3개 inline settle attempt를 따른다. 반환 그래프의 보수적 상한은 root 1 + 3 × (TxRaw 1 + inclusion RPC 2 + absence RPC 16) = **58 objects**이며 각 media byte 상한도 그대로다. 실제 유효 attempt의 inclusion과 absence proof는 동시에 존재하지 않는다. typed root는 262,144B 상한을 따른다. 전체 store 검사가 포함되므로 object 상한은 응답 시간이나 메모리 총량 보장이 아니다.

```rust,ignore
let view = engine.reader().get()?;
let batch_id = view.state["batches"][i]["batch"]["batch_id"].as_str().unwrap();
if let Some(saved) = engine.trusted_recovery_failure(&view.commit, batch_id)? {
    let original = saved.evidence.resolve(
        &saved.resolution_evidence_ref,
        nus_exchange_contract::s3::evidence::TYPED,
    )?;
    // saved.raw == original; saved.resolution_evidence is the decoded original.
    // CLOSE signing and VOID collection consume this existing evidence.
    // Broadcasting still requires the existing current-commit effect gate.
}
```

이 보완 전의 `RejectFinal`은 실패 선택값을 candidate와 결정적 재생에 유지했지만, typed root를 store에 쓰는 시점은 `resolution_evidence_ref`가 포함된 VOID receipt 이후였다. 따라서 receipt 이전 재시작에 원문 출처가 없었다. 이제 새 실패 root와 도달 원문을 기존 `transaction.dev`의 immutable object 저장에 포함하고, 저장 재검증 후 WAL fsync→marker fsync/rename/dir fsync→공개→개발 응답 순서를 따른다. `JournalRecord`, `EngineState`, 계약 schema/해시/cap와 실패 경제 의미는 변경하지 않는다. 추가 private 원문 파일은 기존 object 경로와 no-replace 규칙을 따른다.

`Engine::open`도 원 명령의 semantic replay를 마친 뒤 모든 저장된 실패 선택의 원문·참조 그래프를 확인한다. **이전 후보로 만든 home에서 `RejectFinal`은 있지만 typed root가 없는 경우 새 open은 오류로 끝난다.** 공통 store 검증이 조회·명령·방송 callback 경계에서도 모든 기존 실패 root를 확인한다. root와 descriptor를 함께 삭제해도 다른 batch 조회·새 명령·CLOSE 방송이 이를 우회하지 못하고 writer를 닫는다. 조회나 open으로 root를 생성·재계산·보충하지 않으며 자동 migration은 없다. 원문 파일이 이미 있는 이전 home은 기존 검증을 통과해야 한다. 새 합성 home에서 실행하는 component 범위이며 운영 데이터 migration이나 runtime pin 발급을 포함하지 않는다.

trusted 변경 응답은 승인된 7필드 envelope다. 공개 계정 응답은 [별도 9필드 계약](S3-ACCOUNT-RECEIPT.md)을 사용한다. `development_receipt=LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE`, `durable_ack=false`, `storage_assurance=UNPROVEN_HOST_SPACE`. 내부 rc3 `CommandResult`는 변경하지 않는다. receipt ledger의 `{receipt,command_seq,record_hash,end_offset}`는 독립 시험/인계 컨테이너이며 새 public schema가 아니다. 개발 접수와 chain `COMMITTED`는 별도다. D의 실제 HTTP는 `/dev-local/v1/`·loopback·기존 인증/origin·계정 격리와 묶어 후속 업무에서 검증해야 한다.

VOID audit hash, timeout, NOT_FOUND, CheckTx만으로 보류를 풀 수 없다. 원 정산 확정 실패·다른 시도 전부 종결·raw block/results/TxRaw·원 receipt·같은 높이 C 검증을 기존 `proof.rs`/engine으로 통과해야 CORRECTION을 만들 수 있다. P 재사용과 COMMITTED 역전은 허용하지 않는다.

## 로컬 재현

설치된 Rust/Cargo cache만 사용한다. 아래 `NUS_TEST_TMPDIR`는 검토자가 준비한 쓰기 가능한 임시 디렉터리다. Paperclip 실행에서는 `PAPERCLIP_RUN_SCRATCH_DIR`를 우선 사용한다.

```sh
cargo test --offline --locked --manifest-path exchange/Cargo.toml \
  --features dev-local-demo,fault-injection \
  --test s3_dev_local --test s3_candidates -- --test-threads=1
python3 exchange/scripts/verify-dev-local-evidence.py "$NUS70_EVIDENCE_DIR"
cargo test --offline --locked --manifest-path exchange/Cargo.toml \
  --test s3_accounting --test s3_storage --test s3_journal
cargo check --offline --locked --manifest-path exchange/Cargo.toml \
  --no-default-features --bin nus-s3-local-demo
```

마지막 명령은 feature 누락으로 실패해야 한다. 증거 수집에는 `NUS70_EVIDENCE_DIR=<새 증거 디렉터리>`를 설정한다. 경제 시험은 기존 합성 RPC·공개 시험키와 실제 ML-DSA 서명을 사용하며 개발 store/publisher를 거쳐 실행한다. receipt ledger와 실제 WAL/object/marker 원문을 기록하고 동일 home 두 번 재생을 비교한다. artifact에 복사된 home은 읽기 전용 증거다. 경로/inode를 편집해 재사용하지 않는다.

binary 입력은 `validate|create|open --local-demo-profile <파일> --acknowledge-unproven-space --runtime-pin <독립 인계 hash> --input-set <bundle>`이며 create/open에 `--home`, create에만 `--bootstrap`이 추가된다. bundle은 `runtime_manifest/files/guard/genesis` exact bytes의 base64 JSON이고 private transport 형식이다. binary는 listener·worker를 시작하지 않는다.

성능, 전원 상실, 실제 host ENOSPC/EDQUOT, 물리·metadata 예약/drain은 측정하지 않았다. 오류 주입과 process exit는 각각 모의 IO/component 결과다. 실제 DEV01~14 통합과 D/E/F·보호 main·CI·독립 QA는 NOT_RUN이며 동일 최종 후보의 CTO→Security 승인 후에만 NUS-70을 done으로 인수한다.

## Trusted Seal/Apply 준비 판단

`Engine::trusted_reconcile_readiness(&Commit, &Observation, now)`는 현재 writer와 동일 commit에서 `ReconcileReadiness { commit, seal, apply, applied_height, latest_height }`를 반환한다. `dev-local-demo` 안에서만 사용하는 Rust transport이며 Serialize·REST·CLI route·callback이 없다. 입력 `Observation`은 미적용 관측을 포함한 **latest** snapshot의 id/height와 실제 조회 시각이다. 기존 freshness 상한을 그대로 적용한다.

| 반환 | 의미 / 소비자 행동 |
|---|---|
| `apply = Ready` | 기존 Apply를 비공개 candidate에 실행하고 cap을 확인했다. 먼저 `Command::Apply`를 실행하고 새 commit으로 다시 조회한다. |
| `apply = NoPendingObservation` | Apply할 새 관측이 없다. 불필요한 Apply를 반복하지 않는다. |
| `apply = Held { reason }` | 기존 Apply의 `UNSETTLED_HOLD` 또는 `ATTEMPT_UNRESOLVED`. 자산 해제나 확정 실패를 뜻하지 않는다. |
| `seal = Ready(Normal)` | 기존 FIFO prefix와 12블록 Seal 여유를 통과했다. `Command::Seal(purpose.as_str().into())`로 실행한다. |
| `seal = Ready(ResolveFailure)` | FIFO prefix에 실제 만료·owner epoch/revoke·origin operator epoch 불일치가 있다. 불변 배치를 Seal할 준비일 뿐, 실패/VOID 확정이나 자산 해제 허가는 아니다. |
| `seal = Waiting(EmptyQueue)` | graph에서 pending이며 아직 배정되지 않은 fill이 없다. |
| `seal = Waiting(ExpiryMargin)` | 유효 prefix지만 Seal 여유가 12블록 미만이다. 새 관측까지 기다린다. 오류 후 RESOLVE_FAILURE로 재시도하지 않는다. |
| `seal = Waiting(ApplyPending)` | 유효 prefix와 미적용 관측이 있다. Apply 결과를 먼저 처리한다. |
| `seal = ActiveBatch { batch, state }` | 기존 `active()`에 해당하는 미해소 batch의 사본이다. 새 Seal 대신 기존 attempt/receipt 복구 경로를 사용한다. |

`apply`와 `seal`은 같은 commit의 독립 판단이다. 소비자는 Ready Apply → Ready Seal → ActiveBatch 대사/새 관측 대기 순서를 사용한다. 실제 만료만 관측한 경우 기존 Apply도 가능하다. Apply 후 다시 조회하면 만료된 pending fill에 대해 RESOLVE_FAILURE가 유지된다. 반대로 queued fill의 epoch/revoke 변경에서는 Apply가 Held이고 Seal은 ResolveFailure다. 경제 판단은 C 내부에 남으며 SRE가 FIFO나 만료/epoch 규칙을 복제할 필요가 없다.

Seal 판단은 기존 mutation과 **동일한 private `seal_prefix`**를 사용한다. 최대8 FIFO pending fill, 첫 fill의 origin epoch로 묶는 규칙, owner epoch/revoke/만료·12블록 여유가 그대로다. 선택한 목적만 기존 `seal_batch`에 비공개로 적용해 wire/proof/cap을 검증한다. 한 purpose가 실패한 뒤 다른 purpose를 시도하지 않는다. Apply는 기존 `Candidate::apply`를 그대로 실행한 사본을 버린다. `RECEIPT_INCONSISTENCY` 등 두 보류 코드 이외의 오류는 정상 Wait나 다른 목적이 되지 않고 오류로 반환한다.

조회는 writer lock → 기존 recovery/store gate → expected Commit의 seq/hash/offset 대조 → committed prefix·참조 원문 검증 → freshness·private 판단 → prefix·store 재검사 순서다. prefix 검증은 `Store::open`과 동일한 `scan_committed/read_frame`를 사용한다. 별도 WAL parser가 아니며 marker나 writer의 파일 offset을 변경하지 않는다. 같은 길이의 WAL 내부 변조, 과거/최신 참조 object와 descriptor의 동시 삭제도 감지한다. 감지 시 writer와 reader gate를 RECOVERY_REQUIRED로 닫고 새 WAL/경제 상태/receipt를 생성하지 않는다. `open`의 동일 parser·재생·누락 실패 원문 거절 의미는 유지한다.

`STALE_COMMIT`이면 예전 결과와 부분 복구를 버리고 최신 View/관측으로 다시 조회한다. 이 반환은 읽은 순간의 판단이며 실행권을 예약하거나 방송을 허가하지 않는다. 판단 이후 다른 command가 commit되거나 시간이 지나면 결과를 폐기한다. 실행은 기존 `execute` 검증을 다시 거치고 오류를 catch해 다른 purpose로 우회하지 않는다. `RECOVERY_REQUIRED`, 저장 오류, writer poison에는 준비 판단도 실패한다. 기존 history/attempt/failure 복구·terminal 방송 거절·signed 원 결과 조회는 유지한다.

WAL prefix와 원문 집합을 두 번 읽으며 처리 비용은 보존된 기록량에 따라 증가한다. 페이지 응답, 일정 지연, 처리량을 보장하는 API가 아니다. 큰 원장을 위한 별도 인덱스/성능 변경은 이번 범위에 포함하지 않는다. API 반환에는 새 Batch/TxRaw ID·서명·receipt가 없고 메모리 사본의 수정은 엔진에 영향을 주지 않는다. 시험의 합성 snapshot/test pin을 실제 체인 실행이나 승인 runtime으로 표시하지 않는다.


## F14 correction closure 시험 경계

`dev-local-demo,fault-injection` 전용 `Engine::set_correction_hook`의 정확한 위치, Prepare/SemanticReplay 방문 수와 오류·재시작 의미는 [S3-F14-FAULT-API.md](S3-F14-FAULT-API.md)에 명세했다. 기존 IO hook·경제 규칙·개발 receipt 의미를 유지한다.

## Settlement 방송의 현재 관측 gate — CTO-70-02 수정 후보

`Worker::broadcast`는 같은 reader Commit의 원 Attempt를 pin하고 기존
0/1000/2000ms backoff 뒤 writer lock을 잡는다. commit이 바뀌면
`STALE_COMMIT`으로 거절한다. 같은 writer 아래 prefix/raw·최신 Snapshot의
`freshness` 검증 → UNKNOWN/count+1 저장·공개 → 원 TX/store 재검증 → 현재
시각 freshness 재검증 → bounded callback 순서다. 사이에 다른 engine writer가
진입하지 않는다. 잘못된 관측·backoff 만료·관측 경합은 callback0·새 intent0이다.

production API는 adapter의 `now`뿐 아니라 현재 SystemTime과 진입 후 Instant
경과를 사용한다. 오래된 `now`로 signer/queue/fsync 지연을 숨길 수 없다.
marker가 완료된 뒤 만료되면 callback은 거절하지만 이미 저장된 UNKNOWN/count는
보존한다. 이 경우를 저장 전 거절의 commit0과 구분한다. 자동 rollback/retry·예산
초기화는 없다. `reconcile`과 trusted read의 기존 CATCHING_UP 처리에는 새 일괄
freshness 제한을 적용하지 않는다.

fault-only `test_broadcast_with_clock`은 실제 경로의 clock/barrier 시험용이며
`dev-local-settlement,fault-injection` 두 feature가 필요하다. clock 샘플은
pin/backoff 뒤 writer 전, writer/prefix 검사 뒤 intent 전, marker/raw 검사 뒤
callback 전이다. writer 전 샘플 외의 callback에서 writer 재진입은 금지한다.
기본/no-fault build에는 이 API나 clock override가 없다. 기존 F14 phase는 같다.

**공개 receipt 보완(CTO-70-03):** 승인 A의 별도 `s3-dev-local-account/1`을 구현한 재심사 후보다. 원 trusted `s3-dev-local/1`·전체 CommandResult는 그대로 보존하고, 공개 응답은 검증된 원 명령의 immutable source와 계정별 account_result를 반환한다. 새 Context/home 입력 조건, API와 cap·오류·과거 조회 의미는 [공개 계정 영수증 인계](S3-ACCOUNT-RECEIPT.md)에 있다. 현재 C 후보의 CTO→Security 승인과 최종 runtime pin 전에는 서비스 활성화하지 않는다.
