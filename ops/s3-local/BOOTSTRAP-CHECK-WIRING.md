# 초기화 checker descriptor·승인 연결

`bootstrap_check.check`는 초기화 전용 `bin/s3-local-bootstrap-check`를 캡처된 SRE descriptor에서만 선택한다. 기존 home용 validator로 fallback하지 않는다. 승인 audit → exact 입력 capture → checker SHA 대조/private 0700·0500 사본 → bounded supervisor → 새 audit 순서다. checker 인자는 pin/profile/명시적 공간 opt-in만 구성하며 임의 옵션·home/key 인자를 받지 않는다. 반환값은 home_created=false, approval_verified=false, reusable_permit=false, DEV=NOT_RUN이다. 조직 승인/초기화 허가가 아니다.

기존 offline_check의 private 실행 사본 코드를 내부 함수로 공유했다. 기존 공개 API와 프로세스 감독 계약은 유지한다. 설치/서비스/RPC/listener0, €0. C/B/Wallet 경제 로직 변경0.

검증: `PYTHONPATH=worktrees/NUS-73/ops/s3-local python3 -B -m unittest test_bootstrap_check test_offline_check test_process_check -v` — 신규4+기존11 = 15 PASS/0 FAIL (41.851초). checker exact argv/capture·원본 교체 격리·SHA 변경·경로/opt-in·없는 descriptor 거절, 승인 변경·semantic 오류·interrupt 후 사본 정리. 회귀는 bounded IO/시간/출력·환경 격리·kill/reap을 포함한다. 신규 승인 reader/checker는 합성 script이며 실제 Rust/C의 이번 연결 시험은 미실행이다. 기존 실제 C checker 검증 근거를 이 시험으로 중복 계산하지 않는다.

다음 SRE 작업: 실제 Rust checker 전체 연결 및 초기화 create CLI, chain 관리 launcher/fault/정리, 최종 build/binary/web SHA·다섯 descriptor manifest, CEO/CTO 독립 승인 및 CTO→Security. runtime pin 미발급·DEV NOT_RUN. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false와 부모 blocker 유지.

API 확인: monitorAttemptCount=100, monitorNextCheckAt=null, activeRecoveryAction=null. 소진 monitor 재쓰기/상한 우회0. 동일 업무 adapter/runtime continuation 복구·확인을 SRE 소유 unblock action으로 유지한다. 기본 sandbox API GET 연결 거절은 확대 실행 GET으로 해결했다.

정리: 이번 build cache/binary 생성0. 시험 subprocess/private 사본은 감독 및 fixture cleanup으로 제거했다. 게시 성공 확인 후 run-owned issue.json 사본만 삭제하고 실제 전후 bytes를 댓글에 기록한다. 소스·활성/공유 cache·home·키·원장·검증 로그는 보존한다.
