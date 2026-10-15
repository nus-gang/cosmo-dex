# 웹 승인 준비 연결

NUS-73 · 2026-10-07

reviewed_web.prepare는 인증 audit → 동일 입력 capture → 웹 asset SHA 대조 → 기존 C offline validator → capture guard의 공개 Context → 새 audit 순서를 연결한다. 초기 거절은 capture/validator0, 의미 오류·interrupt·승인 변경은 반환0이다. 입력 경로를 다시 읽어 Context를 바꾸지 않는다. 반환 객체는 listener나 실행 permit을 제공하지 않는다.

신규4+asset 회귀4 = 8 PASS/0 FAIL. 승인 reader/capture/validator는 mock, asset capture와 응답 처리는 실제 Python 함수다. 실제 Rust/C/HTTP/browser 회귀는 이번 미실행. 실제 listener/서비스/RPC0, runtime pin 미발급, DEV NOT_RUN, €0.

남은 SRE 작업: 실제 C 연결·관리 web 시작 gate/CLI·새 fee0/25 home/genesis·chain/fault/정리·최종 build/manifest·독립 승인 및 CTO→Security. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 유지.

현재 monitorAttemptCount=100, monitorNextCheckAt=null, activeRecoveryAction=null. 이번 실행은 활성화됐으나 다음 continuation은 저장되지 않았다. 소진 monitor 재쓰기/상한 우회 없이 SRE 소유 adapter/runtime continuation 복구 경로를 유지한다.
