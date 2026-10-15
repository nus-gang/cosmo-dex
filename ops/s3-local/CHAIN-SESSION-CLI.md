# 인증 Chain session CLI

신규 CLI5 + authenticated Chain 회귀4 = 9 PASS / 0 FAIL. 제어 API/auth/stage/preflight/host probe는 합성이다. 실제 Go/Rust/C 재시험·서비스·START·RPC는 0이다.

`chain_session_cli.py run-chain-reviewed --python ABS_PYTHON --candidate ABS_CANDIDATE --fee-bps 0 --validator-index 0 --workspace-id UUID --duration-seconds 120 -- CHAIN_ARGS`

CHAIN_ARGS는 `chain_cli.py serve-chain-reviewed` 뒤의 정확한 인자 목록이다. fee는 0/25, validator는 0..3, duration은 정규 정수 1..240초다. 기존 등록 packet과 동일 python/candidate/argv 및 workspace UUID를 사용한다. 자동 등록은 하지 않는다. L-R에서는 위 실행 명령을 실행하지 않으며, 승인 pin과 등록 완료 후 L-T에서만 실행한다.

순수 인자 검증 뒤 signal scope→새 Mailbox→현재 run 인증 session→유한 대기→targeted stop→PID/port 관측 순서다. 종료 확인 세 필드가 모두 true여야 고정 보고서를 출력한다. 상세 evidence/인증값은 stdout에 쓰지 않는다. mailbox는 자동 삭제/보관 이동하지 않는다. writer_release_verified/inventory_complete_verified/cleanup_complete_verified는 false이고 DEV는 NOT_RUN이다.

시험: fee0/25 exact argv/validator 전달·유한 대기·종료 보고·중복/축약/잘못된 opt-in/경로/번호 거절·오류/interrupt·시계 역행 종료·초기 stop의 mailbox/session0·열린 stdin 즉시 거절. 기존 인증 session4의 eight selector·철회/변조·stop·부분 증거 보존 회귀 포함.

남은 작업: 전체 topology·fault/정리·최종 build/다섯 descriptor manifest·독립 승인·CTO→Security. runtime pin 미발급. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 유지.
