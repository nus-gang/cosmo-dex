# 관리 웹 실행 진입점 — NUS-73

`python3 -B ops/s3-local/web_cli.py serve-web-reviewed`는 L-T의 Paperclip 관리 command용 foreground 진입점이다. 서비스 실행은 동일 최종 후보의 독립 승인 및 CTO→Security 완료 뒤에만 수행한다. 이번 작업은 실행 코드와 거절 시험이며 실제 서비스 기동은 0회다.

기존 reviewed offline CLI의 필수 인자 전체에 `--approval-socket /absolute/private/broker/s`와 `--web-origin http://127.0.0.1:5173`과 `--pid-mailbox /absolute/private/web-pids`를 추가한다. localhost origin도 허용하지만 listener는 127.0.0.1:5173에만 bind한다. `--bind`는 worker의 IPv4 loopback endpoint이며 웹 포트와 달라야 한다. worker의 input-set/home/key-directory와 검증 인자는 그대로 C 의미 검증에 전달한다. private socket·web origin·PID mailbox는 Rust argv에 포함하지 않는다.

두 opt-in, exact native decision/CEO/CTO revision, absolute 경로, lifetime 1–300초, max-requests 1–10000을 검증한 뒤 signal latch와 private 승인 reader scope를 연다. 기존 managed_web이 audit→동일 capture/의미검증→새 audit→stop 확인→listener 순서를 수행한다. 성공은 stdout 없이 종료하며 오류는 고정 메시지/exit2다. API token을 인자나 파일로 전달하지 않는다.

검증: `python3 -B -m unittest test_web_cli.WebCliTest test_managed_web.ManagedWebTest -v` — 8 PASS/0 FAIL. CLI run/reader는 mock, 기존 lifecycle은 합성 socket이다. 최초 공통 fixture 포트를 잘못 가정한 assertion을 수정하고 재시험했다. 실제 Rust/C/browser/HTTP 통합은 이번 미실행이다.

등록 packet과 web PID/종료 inventory, 실제 C 연결, 새 fee0/25 home/genesis, chain/fault/정리, 최종 build/다섯 descriptor manifest와 독립 심사는 남아 있다. 이 CLI는 runtime 등록 또는 승인 pin 발급 근거가 아니다.

## 웹 PID 기록

L-T orchestrator는 worker용 Mailbox와 별도로 `web_pid_mailbox.WebMailbox(path)`를 생성한다. CLI는 동일 경로의 challenge를 읽고 최종 승인 조회 뒤 listener 생성 전에 자기 PID를 `s3-local-web-pid/1` 원문으로 no-replace/fsync 기록한다. 기록 오류·중단은 socket 생성 전에 거절한다. 웹 프로세스는 child worker가 없으므로 worker용 두 PID 형식을 사용하지 않는다.

관리 stop 확인 후 `WebMailbox.collect()`는 세션 nonce에 결합된 PID 1개 tuple을 반환한다. 디렉터리/원문은 성공·실패 모두 보존하며 자동 삭제나 재사용하지 않는다. 전체 descendant·프로세스 종료·port 해제 입증이 아니며 관리 session 종료 inventory 연결은 아직 남아 있다.

이번 검증: 신규 mailbox/lifecycle4 + CLI/lifecycle/등록 회귀10 = 14 PASS/0 FAIL. 실제 단명 subprocess PID 기록과 메모리 socket 검증이다. 실제 listener/서비스·Rust/C/browser 통합 재시험0.
