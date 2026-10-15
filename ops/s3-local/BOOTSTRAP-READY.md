# 초기화 READY 부모 감독

bootstrap_ready.ready는 bootstrap_stage.stage 내부에서 쓰는 순수 거절 probe다. create-captured 전용 인자 순서를 기존 bounded stdin/socketpair 감독에 전달한다. audit 전후 RPC private 사본 SHA를 대조하고, READY 후 새 audit·실행 binary/capture/RPC bytes·stop을 확인한다. START는 보내지 않고 scope 종료/오류/interrupt에 process group kill/reap을 수행한다. child가 보존한 원문과 home 경로를 삭제하지 않는다.

신뢰 RPC 획득·인증 audit closure는 호출자 책임이다. RPC SHA는 출처 입증이 아니다. 이 반환은 조직 승인이나 재사용 permit이 아니다. 현재 실제 Rust create와 최종 인증/create CLI 연결은 남아 있다.

신규 합성 subprocess3 + 기존 READY4/managed4 = 독립11 PASS. 실행 로그15 중 imported READY4 중복 제외. argv·환경 격리·원문 보존·home 생성0·reap, 승인 변경/RPC 변조/stop/interrupt 거절·사전 오류 spawn0 확인. 기존 managed 회귀의 합성 START를 실제 서비스 시작으로 계산하지 않는다. Rust/C 재시험0·실제 서비스/listener/RPC0·pin 미발급·DEV NOT_RUN. €0, G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 유지.
