# S3-C 기초 모듈 구현·검증 진행 보고

2026-10-05 · Exchange · [NUS-56](/NUS/issues/NUS-56)

**원장·방향성 의존 그래프·S3 WAL 기초 모듈을 구현했다. 전체 엔진 통합은 미완료이며 [NUS-64](/NUS/issues/NUS-64)의 CTO 계약 정정·Security→QA 재심사를 기다린다.** 이 보고는 완료 승인 또는 실제 체인 정산 결과가 아니다.

## 구현 결과

- `Ledger::reconcile`: 최신 전체 C와 terminal fill·종료 잔량을 한 private candidate에서 재계산한다. R/D/P는 원 예약/fill로 합산하며 P를 C에 더하지 않는다. 가격 개선 차액은 COMMITTED 적용 candidate에서 해제된다. 부족 C/owner 누락/terminal 역전은 전체 무효과다. 원 fill과 lifetime matched·settled·pending·corrected 수량을 유지한다.
- `Graph`: 두 주문과 두 owner/epoch/debit asset domain별 최신 선행 pending fill을 저장·재검증한다. 정정은 앞으로 폐쇄하고 같은 owner의 다른 확정 자산을 쓰는 독립 fill과 COMMITTED를 보존한다. 원본 edge·fill ID·순서는 바뀌지 않는다.
- `Journal`: 독립 S3 Context·S3W1, LOCAL_FSYNC, 단일 writer, 전용 correction reserve, marker 이후 반환, monotonic response revision을 구현했다. 원시 proof bytes를 fsync object로 저장하고 WAL ref의 길이·SHA를 append/replay에서 검증한다. unknown tail·누락/손상 증거를 보존하고 복구를 닫는다. 다른 세대 home은 쓰기 전에 거절한다.

상위 sequencer의 원시 receipt·실패 proof 검증·같은 H 결합·book/FIFO·cursor·snapshot 원자 공개를 우회하는 공개 서비스 API는 만들지 않았다. 기초 모듈의 arithmetic/graph 호출 자체는 증거 검증 권한이 아니다.

## 검증 결과

| 검증 | 결과 | 범위/한계 |
|---|---|---|
| S3 accounting/graph | PASS 7 | 0/25bps, 가격 개선, P 재사용 거절, 부분 확정·R 보존, 정정 closure·독립 fill·terminal 불변 |
| S3 journal | PASS 16 | subprocess helper1 포함; writer/증거/손상/용량/revision/namespace |
| crash 재생 | PASS | 실제 파일 IO·process exit 6지점×3회×2재생; 전원 상실 아님 |
| 누적 경계 | PASS | A 합성 내부 이력1000/1001 fills·200/201 orders·0/25bps; API 페이지 절단 없음 |
| S2 ledger/journal 회귀 | PASS 19 | helper1 포함; 원 S2 구현 파일 변경0 |
| clippy all-targets/all-features | PASS | warnings 거절 |
| A 명세 oracle | PASS 13,260 | 상속 계약/fixture/manifest 확인; 제품 QA 아님 |
| 전체 S3 sequencer/원자 EngineState/BatchV2 재구성 | NOT_RUN | 계약 정정 이후 구현·시험 필요 |
| 실제 receipt/4검증인/HTTP 동시성/독립 보안·QA | NOT_RUN | [D](/NUS/issues/NUS-57)·[F](/NUS/issues/NUS-59) 연결 및 후속 인수 |

`raw-results.json`은 테스트가 출력한46개 결과를 보존한다(가격 개선6, 합성 누적경계4, crash/replay36). `tests.log`, `s2-regression.log`, `clippy.log`, `contract-check.log`가 원시 stdout/stderr다. 기대값과 다르면 Rust assertion이 실패하며 성공 행의 `expected_diff=[]`는 그 비교가 통과했음을 뜻한다. 테스트용 F1/F2와 합성 fixture는 실제 batch/TX/체인 fill 증거가 아니다. 실제 genesis/높이/TX/batch는 null/NOT_RUN이다.

최초 테스트 작성 중 타입 추론/출력 macro compile 오류를 수정한 후 위 최종 명령이 통과했다. 런타임 HOME의 rustup 쓰기 거절은 기존 Rust1.92.0 toolchain와 run 전용 Cargo home으로 해소했으며 lock 변경/설치0이다. 테스트 시간은 벤치마크 또는 TPS 측정값으로 사용하지 않는다.

## 계약 결함과 재개 조건

`schema.json`의 `EngineState.corrections[]`는 완전한 `Correction`을 포함하고 `Correction.after_state_hash`는 정정 후 그 상태 hash다. SCHEMA의 ENGINE_STATE hash 규칙에는 이 순환을 끊는 제외/투영이 없다. 정적 의존식은 `hash(state(correction.after_state_hash=hash))`다. 구현자 임의 규칙은 서로 다른 chain/engine/worker 결과를 만들 수 있어 추가하지 않았다. 원 schema 조각과 정확한 출처는 `hash-cycle.json`에 남겼다.

CTO에게 [NUS-64](/NUS/issues/NUS-64)를 배정하고 [NUS-56](/NUS/issues/NUS-56)의 `blockedByIssueIds`에 연결했다. 초기 plan에는 비순환 규범·계산 순서·정정1회/누적2회 fixture·manifest·Security→QA 재심사·승인 SHA 인계를 요구했다. child 생성 직후 네이티브 정책 추가 PATCH는 실행 소유권409로 실패하여 재시도하지 않았다. CTO가 자기 업무에서 재심사 경로를 구성해야 하며, 단순 문서 수정만으로 이 선행 조건을 충족한 것으로 보지 않는다.

Exchange의 다음 단계는 승인 변경을 격리 branch에 반영하고 영속 outbox/단일 미확정 batch, 원 receipt/실패 증거 검증, in-flight 동결, 같은 높이 C·cursor·D/P·R·book/FIFO·정정 revision 원자 적용, 최악 정정 공간 산정, 전체 semantic crash/replay, D API 인계를 완료하는 것이다. 최종 구현 뒤에만 원 [NUS-56](/NUS/issues/NUS-56)의 CTO→Security 네이티브 검토를 요청한다.

## 출처와 전달

GitHub main `bd9e473196ac86fdedf655b2c93e6931f54faa83`, 승인 A head `0915375cac360f83d62a70587a0e7cf9c89604a1`·tree `ad98a57e149b861369f7d716aed8fd0325aef4f8`를 소비했다. 현재 작업은 `exchange/nus-56-s3-reconcile` 격리 checkout에만 있다. 공유 checkout·다른 담당 branch·protocol·lock 변경0, main 병합0이다.

`manifest.json`에 code 파일 hash·A contract/config/vector/lock·명령/환경·결과를 고정했다. commit 이후 생성한 외부 `candidate.json`에 exact head/tree를 기록한다. GitHub branch/commit work product와 Paperclip 업로드 보고서·소스 패치·검증 ZIP으로 전달하며 로컬 경로만 인계하지 않는다. 재현 명령과 모듈 호출 책임은 `exchange/S3-COMPONENTS.md`에 있다.
