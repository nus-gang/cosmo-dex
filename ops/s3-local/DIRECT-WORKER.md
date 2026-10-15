# 직접 TX helper와 worker 수명 연결

NUS-73, 2026-10-07. `direct_worker.py`는 기존 검증 capture의 helper 사본을 worker 종료까지 유지하는 내부 조합이다. `checked_ready`와 `checked_run` 및 Rust argv parser에 연결했다. 검증기는 helper 인자를 받지 않고 worker만 descriptor로 고정한 private helper 인자를 받는다.

- 호출자는 `validated_stage`와 인증된 exact 후보 audit를 소유해야 한다.
- 동일 capture descriptor로 helper bytes를 고정한다. caller의 helper 경로/SHA override는 거절한다.
- worker 생성 전, READY 뒤, START 직전 audit 및 helper SHA를 재검사한다.
- child kill/reap 뒤에만 private helper 사본을 제거한다. 원본 artifact·home·키는 삭제하지 않는다.
- `ready_with_direct`는 START를 보내지 않는다. `run_with_direct`는 L-T용이며 L-R에서는 합성 IPC child로만 시험했다.

검증: 신규5 + 기존 사본 시험4 = 9 PASS / 0 FAIL. 실제 합성 subprocess로 helper argv/SHA·원본 교체 격리·READY 동안 사본 존재·START 직전 변조/철회/interrupt 거절·child 회수/사본 제거를 확인했다. 합성 START1 경로는 실제 서비스 실행 근거가 아니다. Go helper/Rust/C 재시험0·Chain RPC/서비스0·runtime pin 미발급·DEV NOT_RUN.

남은 연결: worker의 direct_child 호출 및 인증 방송/결과 HTTP, browser ChainPort, 새 home/genesis, chain/web/fault/정리, 최종 manifest·독립 승인·CTO→Security. 전체 descendants/cleanup 보장과 조직 승인으로 확대하지 않는다.


2026-10-07 launcher 연결 검증: 실제 Rust worker/validator/startup 컴파일 PASS. fee0/25 descriptor에 실제 Go helper SHA를 넣고 capture→의미검증→private helper/worker→READY를 검증했다. 조직 승인 reader만 합성이며 서비스 START0. 정상 READY와 READY 후 합성 철회에서 child reap·사본 제거, writer 재개방 및 두 번 replay/commit 불변을 확인했다. 단일 Rust 연결시험 PASS(55.60초); 같은 binary의 나머지276 시험은 미실행. Python 신규 checked 진입점 배선1+기존 direct worker5 및 사본4 = 독립10 PASS. 합성 child의 START는 실제 서비스 기동이 아니다.
