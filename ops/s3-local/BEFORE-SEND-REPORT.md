# F05 예약 기록 검사

`python3 ops/s3-local/before_send_report.py inspect /absolute/private/report-root EXPECTED_BOUNDARY_SHA256`

기존 writer의 `before-send.jsonl`을 읽기 전용으로 검사한다. 기대 SHA는 별도 보존한 callback 원문에서 얻는다. 두 기록의 exact 원문/SHA, F05·SUBMISSION_UNKNOWN·TX/intent 해시·count 1..3·TX 길이 상한과 비허가 표시를 확인한다. private root0700/file0600·링크/경로 교체 검사는 기존 bounded reader를 공유한다.

예약만 존재하거나 final이 잘리면 UNKNOWN. 완전한 final은 boundary_returned/boundary_error/panic을 그대로 보고한다. 이는 기록 일관성 검사이며 실제 방송 부재·crash·fsync·명령 성공·replay·DEV 또는 조직 승인을 입증하지 않는다. 실제 Worker writer→reader는 fee0/25 정상/오류/crash 기록을 교차 검증했다(기존 근거 fdf190c7-91d5-43b8-af7a-64d4e0be9366). 인증 실행 종단과 DEV 검증은 아직 NOT_RUN. 손상 파일을 수정하거나 재시도하지 않는다.

## 전용 child 입력 준비

`runtime/before_send_options.rs`는 `--enable-f05-before-send true --tx-hash <lowercase SHA256> --fault-evidence-root /absolute/private/path --worker-inputs <worker arguments>`를 읽는 순수 parser다. 경로 조회/생성·intent 저장·방송은 하지 않는다. 두 개발 opt-in·승인 gate는 기존 worker/부모 경로의 책임이며, 전용 child와 인증 부모 경로에 연결돼 있다. errno/phase/command/occurrence 혼합과 비정규 hash·경로를 거절한다.

## F05 READY 부모 경계

`before_send_ready.ready`는 `before_send_stage.stage` 안에서만 사용한다.
명시적 enable, 64자리 소문자 TX SHA256, 정규 절대 evidence 경로를 받고
`f05-crash-captured`에 정확한 옵션/worker 구분자와 capture SHA를 전달한다.
READY 뒤 최신 승인과 실행/capture bytes를 다시 검사하며 scope 종료 시 kill/reap한다.
START를 전송하지 않고 evidence root를 만들지 않는다. READY는 F05 검증이나 재사용 허가가 아니다.

`before_send_run.run`은 READY 뒤 세 번째 audit와 executable/capture bytes·stop·생존·기한을 다시 확인한 다음에만 정확한 `START\n`과 EOF를 보낸다. exit 86은 `UNKNOWN`이며 transport/F05/crash/replay 승인으로 바꾸지 않는다. 출력·다른 종료·시간 초과·중단은 모두 결과 불명으로 거절하고 child를 kill/reap한다. `before_send_cli.py run-reviewed`는 같은 stage와 signal scope에서 이 one-shot 경계를 호출한다.

`PYTHONPATH=ops/s3-local python3 -m unittest test_before_send_run test_before_send_run_cli test_before_send_ready test_before_send_cli`
실행 감독4+실행 CLI4+READY4+READY CLI5 PASS. 합성 subprocess/mock 승인 시험이며 실제 Rust/C 경계와는 아래 시험으로 따로 연결했다.

`test_real_before_send.py` 연결 시험은 fee0/25의 실제 preflight/F05 child를 READY까지 준비한 뒤 최종 audit 철회·capture 변조·stop·interrupt 각각을 주입한다. 모든 경우 exact `START\n` 전에 거절되며 evidence root 생성과 원 입력 변경이 없고 private 사본·writer가 정리된다. `offline_descriptor_capture_real_validator` 1 PASS/0 FAIL, 387 filtered, 100.60초. 이는 실제 child의 시작 직전 거절 경계 근거이지 transport 호출·F05 주입 성공·DEV 인증이 아니다.
