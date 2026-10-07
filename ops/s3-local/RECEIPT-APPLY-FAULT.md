# F09 Receipt→Apply 경계

`runtime/receipt_apply.rs`는 fault build에서만 사용하는 단일 실행 경계다.

- caller가 승인 L-D의 exact `Receipt` 영속화를 한 번 실행한다.
- C commit과 `resolution_receipts`가 정확히 하나 증가했는지 확인한다.
- `accounts`, `fills`, `chain_snapshot`, `corrections`가 아직 변하지 않았는지 확인한다.
- private 0700 root에 `receipt-apply.jsonl`을 no-replace 0600으로 만들고 file/root fsync 뒤 action을 호출한다.
- 보고서는 `apply_called=false`, `crash_verified=false`, `durable_ack=false`, `DEV=NOT_RUN`을 명시한다.

이 경계는 receipt를 합성하거나 Apply를 호출하지 않으며 재사용 permit이 아니다. 현재 component 시험은 fee0/25에서 실제 C/L-D Receipt 저장, 자산/정정 미적용, 두 번 reopen, 보고서 원문을 검증한다. crash child, 인증 CLI, 계약의 3회 반복과 L-T의 실제 DEV09 판정은 남아 있다.

`receipt_apply_report.py inspect <absolute-0700-root> <boundary-sha256>`은 보고서를 읽기 전용으로 검사한다. exact boundary/receipt/batch/disposition과 증가한 commit을 결합하고, 예약만 있거나 잘린 final은 `UNKNOWN`으로 남긴다. 완전한 final도 기록 무결성만 확인하며 Apply·crash·receipt 내구성·fsync·replay·DEV를 인증하지 않는다.

격리 component 시험은 Receipt commit 뒤 boundary callback에서 즉시 `exit(86)`하고,
부모가 child를 reap한 다음 같은 home을 두 번 연다. 두 재개방 모두 Receipt 1개와
같은 state/commit을 복원하고 accounts/fills/chain_snapshot/corrections는 Receipt 전과
같아야 한다. 보고서는 예약 행 하나만 남아 reader 결과가 `UNKNOWN`이어야 하며,
이는 crash 지점의 관측 근거이지 Apply 성공이나 DEV 통과 판정이 아니다.

`runtime/receipt_apply_main.rs`는 승인된 capture와 기존 C/L-D API만 여는 전용 관리
child다. `receipt_apply_stage.py`가 descriptor에 결합된 binary를 private 0700/0500
사본으로 만들고, `receipt_apply_ready.py`는 READY까지만, `receipt_apply_run.py`는
세 번째 승인·binary/capture 재대조 뒤 exact `START\n`과 EOF를 한 번 전달한다.
`receipt_apply_cli.py`는 `check-ready-reviewed`와 `run-reviewed`만 허용하며 다음 전용
옵션을 요구한다.

```text
--enable-f09-receipt-apply --batch-id <64-lower-hex> \
--fault-evidence-root <absolute-canonical-path> --worker-inputs ...
```

F05/F14/일반 storage fault 옵션과 섞인 입력, 비정규 경로·hash, 승인 철회,
capture/binary 변경, stop/interrupt는 START 전에 거절한다. child는 관측을 한 번만
수집하고 승인 `CommittedReceipt`/`VoidReceipt` 저장 뒤 위 F09 경계에서 exit 86한다.
부모 결과는 항상 `UNKNOWN`, `apply_called_verified=false`, `crash_verified=false`,
`durable_ack=false`, `DEV=NOT_RUN`이다. 이번 L-R 검증은 child 컴파일과 START 전
거절/정리까지만 수행했다. 유효 START 뒤 실제 child crash/replay의 관리 종단,
계약상 경계별 3회 반복과 L-T의 DEV09 판정은 여전히 NOT_RUN이다.
