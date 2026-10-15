# PID session 연결 — NUS-73

`mailbox_session.session`은 사전에 만든 `Mailbox`의 경로와 exact 관리 argv의
`--pid-mailbox`를 대조한다. capture/artifacts/validator argv는 시작 전에 복사한다.
worker listener는 생성된 exact command에서 추출하며 Chain RPC는 포함하지 않는다.

기존 관리 session이 stop operation과 fresh workspace 조회를 확인한 뒤 mailbox를
한 번 수집하고, 두 PID를 기존 process→port→writer 관측에 전달한다. 누락·부분
기록·nonce 불일치는 기존 Mailbox의 거절을 유지한다. stop 미확인 시 수집하지 않는다.
오류/interrupt를 포함한 모든 경로에서 challenge/pids.json을 삭제하지 않는다.

현재 경계는 same-uid trusted caller용 내부 API다. mailbox 생성→등록 packet→실제
관리 session을 묶는 최종 orchestration/명령은 아직 남아 있다. 전체 descendants,
지속 포트 예약, cleanup 완전성, runtime 승인을 증명하지 않는다.
`inventory_complete_verified=false`, `cleanup_complete_verified=false`를 유지한다.

검증: `PYTHONPATH=ops/s3-local python3 -m unittest
 test_mailbox_session.MailboxSessionTest test_session_release.ReleaseSessionTest -v`
신규5+회귀4 = 9 PASS/0 FAIL. fee0/25, capture 입력 변경, 누락, stop 실패,
잘못된 mailbox 경로, interrupt 후 수집/기록 보존. 제어 API/host probe는 합성이며
실제 서비스/Chain RPC/관리 start·stop 0. Rust/C 재시험0. 최초 PYTHONPATH 누락,
fixture의 변경 command 미반영/예상 port 오류는 수정 후 재시험했다.

## 생성부터 session까지

`prepared_session.PreparedSession(python, candidate, argv, fee_bps=0|25)`는
exact argv/config를 먼저 검증하고 `--pid-mailbox` 위치에 새 private Mailbox를
no-replace로 만든다. `packet`은 권한 있는 등록 경로에 인계할 독립 복사본이다.
등록 API를 호출하지 않는다. 기존 경로 재사용/초기화/증거 삭제는 하지 않는다.

등록 완료 후 동일 객체의 `session(raw=..., artifacts=..., arguments=...,
scratch=..., validator_sha256=..., broker_root=..., broker=..., client=...,
audit=...)`가 기존 mailbox session에 연결된다. 승인된 L-T 호출자 전용이며
이 업무에서 실제 호출하지 않는다. 최초 진입 시 소모되므로 실패·거절·중단도
같은 객체의 재실행을 허용하지 않는다. `evidence_path`의 challenge/PID 기록은
성공/실패와 무관하게 보존한다. packet 수정은 내부 명령에 영향을 주지 않는다.

새 프로세스에서 준비 객체 복원, 반복 시연의 새 mailbox와 고정 등록 command를
연결하는 생명주기, 실제 인증 client/broker/audit 조립 및 최종 CLI는 남아 있다.
이 준비 객체만으로 매 실행의 host-command 등록 권한 문제가 해결된 것은 아니다.
등록 정책 우회·API write·개발 서비스 시작은 수행하지 않았다.

이번 검증: `PYTHONPATH=ops/s3-local python3 -B -m unittest
 test_prepared_session.PreparedSessionTest -v` — 신규4 PASS/0 FAIL.
fee0/25 exact 등록 요청→session→stop 뒤 수집, packet 복사, 중복/잘못된 입력,
오류/interrupt 및 override 후 재호출 거절과 증거 보존. 제어 API/host probe는
합성이다. Rust/C 재시험0·DEV NOT_RUN·runtime pin 미발급.

## 현재 run 인증 조립

`authenticated_session.session(prepared, workspace_id)`는 등록 완료한 동일
`PreparedSession`의 argv에서 승인 판정/revision·input-set·worker 인자를 얻는다.
현재 run의 RuntimeClient, 인증 audit, 고정 private broker를 연결한다.
audit→byte capture→동일 capture 의미 검증→재대조 뒤 관리 session에 들어간다.
관리 start 직전에도 capture 원문과 audit를 다시 비교한다. 종료 writer 관측은
최초 capture와 실제 validator digest를 사용한다. 입력/adapter override는 없다.

이 함수는 별도 L-T 승인 후에만 호출한다. 등록 API/서비스를 자동 실행하는 CLI는
아니며, 이 작업에서는 mock 제어 API만 실행했다. worker의 독립 READY/START gate는
유지한다. 거절/중단도 인증 진입을 소모하며 mailbox 증거를 보존한다. host command
등록 권한·전체 descendants 검증·최종 CLI·웹/chain 초기화와 manifest 심사는 남는다.

검증 명령:
`python3 -B -m unittest test_authenticated_session.AuthenticatedSessionTest -v`
신규 합성4 PASS/0 FAIL: fee0/25 인증 조립과 stop/관측, 첫 승인/의미검증/원문
변경/철회 거절 start0, 최종 audit 거절 start0, interrupt 뒤 stop1회·기록 보존.
실제 Rust/C·인증 API·관리 start/stop 재시험0. DEV NOT_RUN·runtime pin 미발급.
