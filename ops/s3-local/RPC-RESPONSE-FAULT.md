# F06/F07 Settlement RPC 응답 경계

L-R의 기본 비활성 준비 산출물이다. 실제 서비스·socket·RPC·방송 실행과 DEV 판정은
승인 runtime pin 이후 L-T에 남는다.

`runtime/rpc_response_fault.rs`는 승인 L-D `Worker::test_broadcast`의 영속 intent
callback만 사용한다. Worker가 exact TxRaw를 `SUBMISSION_UNKNOWN`,
`broadcast_count=1..3`으로 C store에 commit한 뒤에만 다음 한 경계를 선택할 수 있다.

- F06: request bytes가 transport callback에 전달됐으나 response header를 받지 못한 관측
- F07: 완전한 HTTP header 뒤 non-empty JSON prefix만 받은 관측

두 경계는 request/header/prefix의 SHA256과 길이만 canonical 원문에 결합한다. 응답을
decode하지 않고 Attempt·Receipt·Apply·correction을 만들지 않는다. `rpc-response.jsonl`은
private root0700 아래 file0600/no-replace로 생성하며 reserved/final 각 행마다 file/root를
fsync한다. action 오류와 panic도 최종 phase로 기록하고 driver 재사용을 거절한다.

이 driver는 임의 socket client가 아니다. 보고서와 reader는
`socket_verified=false`, `response_complete=false`, `attempt_resolved=false`,
`asset_effect_verified=false`, `crash_verified=false`, `durable_ack=false`,
`DEV=NOT_RUN`을 고정한다. transport callback 호출은 실제 방송·header 수신·partial read를
입증하지 않는다. 직접 사용자 응답 유실 `response_loss.py`와도 합산하지 않는다.

`rpc_response_report.py`는 exact canonical boundary/SHA, F06/F07 phase shape,
TX/request identity, count/size cap, reserved/final 일치를 읽기 전용으로 검사한다.
부분 final은 UNKNOWN이며 authenticity/fsync/replay/permit를 인증하지 않는다.

현재 component 검증:

- Python reader 4 PASS/0 FAIL: F06/F07·부분 기록·변조/중복 JSON·권한/hardlink·열린 stdin 거절.
- 실제 Rust/C-L-D targeted 시험 1 PASS/0 FAIL, 188 filtered: 시험 내부 fee0/25 ×
  F06/F07. writer lock 중 report 예약, 오류 뒤 accounts/batches 불변,
  `SUBMISSION_UNKNOWN/count1`, 같은 home 두 번 reopen과 writer→Python reader 원문 불변을
  확인했다.

실제 socket에서 해당 byte 경계를 강제하고 child exit86 뒤 3회·두 번 replay 및 원 개발
receipt와 대조하는 일은 수행하지 않았다. 따라서 F06/F07과 DEV09는 아직 NOT_RUN이다.
