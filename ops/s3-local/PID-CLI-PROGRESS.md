# PID 전달 CLI 연결 — NUS-73

관리 serve-reviewed CLI는 필수 --pid-mailbox 절대 경로를 받는다. 사전에 세션이 만든 root0700/challenge를 reporter로 검사하고 on_spawn으로 launcher/worker PID를 기록한다. worker 인자에는 이 경로를 전달하지 않는다. runtime_config도 같은 parser로 옵션을 고정한다.

신규 3 + CLI/config/transport 회귀 8 = 11 PASS/0 FAIL. 실제 합성 READY child의 PID 일치·reap·기록 보존, 누락/중복/상대/없는 경로의 run0, 중단 후 기록 보존 확인. 서비스 START0, 실제 Rust/C 재시험0. 시험은 run을 합성 READY 감독으로 대체하며 조직 승인/전체 정리 인증이 아니다.

L-T 세션은 시작 전에 Mailbox를 생성하고 고정된 관리 command의 해당 경로에 두어야 한다. challenge/PID 기록은 stop 및 종료 관측이 끝날 때까지 보존한다. 누락·부분 기록은 자동 수리/삭제하지 않는다. 현 CLI는 mailbox를 생성·삭제하지 않는다. 재사용 세션 수명/수집과 release inventory 연결은 다음 구현이며 전체 descendant 목록이 입증되지 않았다.

남음: session 수집/증거 생명주기, 웹 ChainPort, 새 fee0/25 초기화, chain/web launcher, fault/정리, 최종 manifest, 독립 CEO/CTO 및 CTO→Security 심사. runtime pin 미발급·DEV NOT_RUN·€0. 원 G00/ACK와 부모 blocker 유지.
