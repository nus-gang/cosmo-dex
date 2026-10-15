# F14 correction closure 시험 API

NUS-70 → NUS-73 · 기존 `S3-DEV-LOCAL.md` API에 더하는 trusted component 시험 전용 경계다. `dev-local-demo,fault-injection` 두 feature가 모두 켜진 macOS/Linux build에서만 존재한다. 계약·wire·경제 규칙·기본 feature·CLI·REST·환경 fallback을 추가하거나 변경하지 않는다.

```rust
use nus_exchange_contract::s3::dev_local::{CorrectionPhase, Engine, Error};
use std::sync::Arc;

fn arm_f14(engine: &Engine) -> Result<(), Error> {
    engine.set_correction_hook(Some(Arc::new(|boundary| {
        if boundary.phase == CorrectionPhase::Prepare {
            // driver가 도달 증거를 남기고 여기서 barrier/crash를 수행한다.
            return Err(Error::Recovery("F14_INJECTED"));
        }
        Ok(())
    })))
}
```

`Engine::set_correction_hook(Option<CorrectionHook>)`는 인스턴스의 다음 execute에 사용할 callback을 설정/해제한다. `CorrectionHook = Arc<dyn Fn(&CorrectionBoundary) -> Result<()> + Send + Sync>`이며, `CorrectionBoundary`는 `phase`, `root_fill_ids`, `corrected_fill_ids`, `surviving_fill_ids`의 사본이다. 사본은 시험 도달 근거이고 엔진 입력·receipt·방송 허가가 아니다. 경로·원문 조회 권한도 주지 않는다.

정확한 위치는 기존 `Candidate::apply`의 `n.graph.closure(&roots)?` 직후다. VOID가 있을 때만 호출하며 unresolved 검증·affected/cancel 구성·`graph.corrected`·ledger/book·CorrectionRecord 변경보다 앞선다. 기존 계산을 그대로 사용한다. 그 이전의 private `next()` 및 committed graph 표시는 기존 알고리즘의 입력 준비이며 공개 상태를 바꾸지 않는다. 외부에서 closure를 다시 계산하지 않는다.

| phase / 실행 | 방문 수와 저장 경계 |
|---|---|
| `Prepare` | 실제 `Engine::execute(Command::Apply)`의 첫 `input.apply()` 안에서 1회. `Prepared::prepare`, 원문 transaction, WAL append, marker, 공개, 응답보다 앞선다. SRE F14 driver가 선택할 phase다. |
| `SemanticReplay` | prepare 성공 뒤 원문 저장·재검증을 거친 `Prepared::replay`의 Apply 안에서 1회. WAL append·marker·공개·응답보다 앞서지만 `transaction.dev`는 이미 존재한다. |
| 정상 VOID execute | 순서대로 Prepare 1 + SemanticReplay 1 = 총 2회. |
| Prepare 오류/강제 종료 | Prepare 1회, SemanticReplay 0회. |
| SemanticReplay 오류/강제 종료 | Prepare 1회, SemanticReplay 1회. |
| VOID 없는 Apply, no-op, closure 이전 거절 | 0회. |
| `Engine::open`, trusted readiness, 직접 Candidate 계산 | 저장된 callback을 적용하지 않아 0회. 재시작은 새 Engine에 hook을 다시 설치해야 한다. |

callback은 writer lock 아래 현재 실행 스레드에서 동작한다. barrier에서 `ReadView::get`으로 원 공개 revision을 검사할 수 있다. callback 안에서 execute·trusted 조회·hook 설정 등 writer lock을 다시 잡는 API를 호출하면 교착하므로 호출하지 않는다. 중첩 경제 계산도 callback에서 수행하지 않는다. scope는 반환·오류·unwind 시 이전 thread-local 값을 복원한다. 기존 문자열 IO hook 및 `candidate_verified`, `before_publish`는 별도 경계이며 F14의 대체 이름이 아니다.

callback이 `Err`를 반환하면 새 성공 receipt/commit/state 공개 없이 writer와 effect를 닫는다. 주입한 `Invalid(code)`도 `Recovery(code)`로 바꾸며 IO 오류는 원 OS 값을 보존한다. 일반 입력 검증 오류는 기존 Invalid 의미를 유지한다. 이후 명령·출금 준비·방송·trusted 복구 조회는 기존 closed gate를 적용하고 원 signed 결과의 읽기 전용 조회는 유지한다. callback panic은 기존 writer mutex poison 규칙을 따른다.

Prepare에서의 반환 오류 또는 SIGKILL은 새 transaction/WAL 쓰기 전이므로 기존 complete prefix로 같은 home을 다시 열 수 있다. closure hook으로 오류가 났던 살아 있는 Engine은 닫힌 채로 남는다. 재시작 뒤 원 결과·원 WAL·원문을 복원하고 Apply를 다시 실행한다. SemanticReplay에서 멈추면 기존 `transaction.dev`가 남으므로 두 번 open 모두 RECOVERY_REQUIRED다. 자동 삭제·truncate·재계산·guard 재발급은 하지 않는다. 이것을 성공 replay 또는 자동 복구로 표시하지 않는다.

검증은 `tests/support/f14_correction.rs`의 합성 RPC·공개 시험 seed·실제 개발 store를 쓴다. fee0/25에서 VOID F1과 epoch 변경으로 root가 된 F2, 의존 F3, 독립 F4를 구성한다. 정상/관측/missed selector의 같은 결과·commit, phase별 reader barrier, 반환 오류, 실제 자식 프로세스 SIGKILL, 같은 home 두 번 open 및 Prepare 재시작 후 동일 correction을 검사한다. 정확한 명령·원시 증거·최종 head/tree·심사 리비전은 해당 제출 인계서에 고정한다.

`G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false`를 유지한다. 실제 DEV01~14·서비스/START/RPC·전원 장애·host 공간 보장·최종 runtime pin·main/CI 완료 근거가 아니다. 이전 후보의 승인 기록은 보존하고 새 exact 후보는 CTO→Security 순서로 재심사한다.
