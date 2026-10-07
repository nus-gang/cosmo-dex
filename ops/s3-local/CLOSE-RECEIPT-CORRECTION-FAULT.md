# F13 CLOSE Receipt → correction plan 경계

2026-10-07 · SRE · L-R 준비 범위. 실제 서비스·listener·RPC·방송·Apply는 실행하지 않았다.

`runtime/close_receipt_correction.rs`는 승인 C/L-D의 기존 `SubmitLane::void_receipt_with`가 원 VOID Receipt를 영속한 다음, correction plan 또는 replacement batch를 만들기 전 경계를 고정한다. 새 경제 전이나 proof를 구현하지 않는다.

경계 진입 조건:

- 원 batch id가 exact 64자리 lowercase SHA256이고 저장 전 상태가 `CLOSING`, receipt가 null이다.
- 저장 뒤 Receipt 수와 commit만 증가한다.
- 원 batch 수와 correction 수가 그대로이고 원 batch는 `CLOSING/ENGINE_APPLY_PENDING`, receipt disposition은 `VOID`다.
- `accounts`, `fills`, `chain_snapshot`은 바뀌지 않는다.

경계 보고서는 private root0700 아래 `close-receipt-correction.jsonl`을 file0600/no-replace로 생성하고 file/root fsync 뒤 callback을 호출한다. 원 boundary bytes/SHA, before/after commit·batch/correction count를 묶고 `correction_plan_visible=false`, `replacement_seq_created=false`, `apply_called=false`, `crash_verified=false`, `durable_ack=false`, `DEV=NOT_RUN`을 고정한다.

`close_receipt_correction_report.py`는 canonical 원문/SHA, commit 증가, batch/correction count 불변, `CLOSING/ENGINE_APPLY_PENDING`, reserved/final 결합을 읽기 전용으로 검사한다. 부분 final은 `UNKNOWN`이며 Receipt 내구·실제 fsync·crash·replay·조직 승인 또는 재사용 permit을 인증하지 않는다.

검증 결과:

- Rust 1.92.0, 기존 offline/locked rlib로 `submit.rs` 시험 binary 컴파일 PASS.
- 실제 C/L-D targeted component 1 PASS / 0 FAIL / 190 filtered. 단일 시험 내부에서 fee0/25, 각 같은 home 두 번 reopen, Rust writer→Python reader 교차 검증을 수행했다.
- Python reader 4 PASS / 0 FAIL. correction/replacement/Apply claim, batch/correction count 변경, commit 비증가, duplicate JSON, 권한/hardlink를 거절한다.
- 최초 Cargo 실행은 현재 shell의 Cargo index 위치가 비어 `bech32` 해석 전에 실패했다. 기존 cache를 명시한 다음 전체 integration target의 기존 `libc` 선언 공백으로 실패해, 이전 검증과 동일한 direct `rustc --test` 경로로 보정했다. dependency download, manifest/lock 변경은 없다.

이 결과는 F13의 component 경계 준비다. 실제 crash child·인증 CLI·계약상 3회 반복과 L-T DEV09 판정은 `NOT_RUN`이다. runtime pin 미발급이며 `G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false`를 유지한다.
