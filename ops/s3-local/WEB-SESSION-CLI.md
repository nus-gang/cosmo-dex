# 인증 web session CLI — L-T 인계 준비

`python3 -B ops/s3-local/web_session_cli.py run-web-reviewed --python <absolute-python> --candidate <absolute-candidate> --fee-bps 0 --workspace-id <registered-workspace-uuid> --duration-seconds 120 -- <web_cli.py serve-web-reviewed의 전체 인자>`

25bps는 별도 등록 workspace와 별도 home/입력을 사용한다. 뒤 인자는 기존 `prepare_web` packet과 바이트가 같아야 한다. 두 opt-in, 승인 decision/revision, manifest pin, 입력 집합, private broker socket, PID mailbox 및 web origin을 생략할 수 없다.

CLI는 순수 인자/등록 packet 검증 후 새 WebMailbox를 만들고 현재 run 인증 orchestration을 호출한다. 이미 존재하는 mailbox는 덮어쓰지 않는다. 실행 시간은 1–240초이며 SIGINT/SIGTERM 또는 종료 시 기존 targeted stop과 PID/port 관측을 수행한다. 성공 출력은 세 관측 여부만 포함하며 cleanup 전체 인증은 false다. 실패는 고정 오류와 exit2이고 비밀/내부 오류를 출력하지 않는다. 모든 경로에서 mailbox를 자동 삭제하거나 재준비하지 않는다.

이 명령은 승인 pin과 관리 등록이 완료된 L-T에서만 실행한다. 이번 시험은 session mock 및 기존 합성 orchestration이며 실제 API start/stop·listener·Chain RPC 실행은 없다. 실제 C 준비 연결·새 genesis/home·chain/fault/최종 manifest와 독립 심사는 아직 남아 있다.
