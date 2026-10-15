# 초기화 승인 재확인 순서

`runtime/bootstrap.rs::initialize_with`는 중단 확인 → exact Validated 입력의 audit → 중단 확인 → 단 한 번 fetch → 원 RPC file/root fsync 보존 → 중단 확인 → 같은 입력의 새 audit → 중단 확인 → 승인 C create를 연결한다. 조회 중 중단/철회가 발생해도 수신한 원문은 보존하고 home을 생성하지 않는다. 오류에 자동 재시도·정리·수리 없음. 호출자에게 반환되는 Engine은 C writer lock을 소유한다.

이 함수는 내부 조합 경계다. audit callback은 인증된 최신 승인 조회를 실제로 연결해야 하며 callback 성공 자체를 조직 승인으로 간주하지 않는다. `Validated`의 동일 입력을 두 번 전달하지만 승인 서버와 파일 생성의 분산 원자성을 보장하지 않는다. 실제 인증 reader/초기화 CLI 결합과 L-T 서비스 실행은 남아 있다. fetch는 L-T에서만 실제 RPC를 사용한다.

시험은 합성 audit/fetch·실제 C store를 사용한다. fee0/25 순서·exact 원문 보존·두 번 replay, 첫/둘째 승인 거절·조회 오류·각 중단 지점·기존 증거 충돌의 home 생성0/조회 최대1회를 확인한다. 기존 bootstrap/query 회귀도 실행한다. 실제 서비스·listener·RPC0, runtime pin 미발급·DEV NOT_RUN. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 유지.
