# F10 Apply candidate 검증 → WAL 전 경계

2026-10-07 · SRE · L-R 준비 범위. 실제 서비스·listener·RPC·방송은 실행하지 않았다.

승인 C의 기존 Apply 경로는 semantic replay와 candidate 일치 검증 뒤 `candidate_verified` hook을 호출하고, 그 다음에만 WAL append를 시작한다. SRE driver는 이 기존 hook 이름을 바꾸거나 경제 로직을 복제하지 않고 `SubmitLane::fault_apply_recorded`의 exact `Apply` 명령으로 한 번만 Generic IO를 주입한다.

targeted component 시험은 fee0/25 각각에서 COMMITTED Receipt를 먼저 영속한 뒤 다음을 확인한다.

- private root0700/file0600 no-replace 보고서가 exact command bytes/SHA, Context, Snapshot, 현재 commit, `candidate_verified`/occurrence=1을 결합한다.
- hook 오류 뒤 공개 reader는 `RECOVERY_REQUIRED`로 닫히지만 state와 commit은 Receipt 직후 값 그대로다.
- `journal.dev.wal`과 `commit.dev.json`의 bytes는 hook 전후 동일하다.
- 이 hook은 object transaction 예약 뒤이므로 `transaction.dev`를 자동 삭제하지 않는다. 같은 home을 두 번 다시 열면 원 transaction bytes를 보존한 채 `UNKNOWN_OR_INCOMPLETE_STORE`로 닫힌다.
- 보고서는 `durable_ack=false`, `DEV=NOT_RUN`을 유지한다.

이 결과는 F10의 정확한 component 경계와 fail-closed 공개 상태를 준비한 근거다. `_exit(86)` crash child, hook 안에서 동시에 읽는 별도 reader barrier, 계약상 3회 반복, 실제 UI receipt ledger 대조와 L-T DEV09 판정은 아직 `NOT_RUN`이다. runtime pin은 발급하지 않는다.
