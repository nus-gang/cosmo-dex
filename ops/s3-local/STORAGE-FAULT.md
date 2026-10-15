# 저장 오류 주입 선택기

`runtime/storage_fault.rs`는 C의 기존 `Engine::set_fault_hook`과 타입이 맞는
SRE 내부 adapter다. 기존 `dev-local-demo,dev-local-settlement,fault-injection`
rlib로 API 연결을 컴파일했다. worker/CLI에는 아직 연결하지 않았다.

두 opt-in, C의 정확한 point 이름, 1..1024번째 해당 point 방문을 명시한다.
전체 방문은 65,536회로 제한한다. 지정 방문에서 합성 IO 오류를 한 번 반환하고
이후 모든 방문을 거절한다. 알 수 없는 point/상한 초과도 닫힌다. finish는
추가 방문을 닫고 숫자와 injected만 반환한다. TX/키/잔고는 기록하지 않는다.

후속 driver는 새 격리 fault-build Engine의 정확한 명령 하나에 hook을 설치하고,
성공/오류/panic 모두에서 hook을 제거해야 한다. 명령 ID·commit·원문 증거와
보고서를 별도로 결합해야 한다. 자동 재시도/환경 변수/프로세스 kill은 없다.
기존 hook이 있는 Engine에 덮어쓰거나 다수 명령을 동시에 실행하면 안 된다.

현재 검증은 순수 selector 4 PASS/0 FAIL 및 C hook API 타입 검사다.
실제 Engine IO 오류·복구·자산 보존·crash·F01~18/DEV 판정은 NOT_RUN이다.
주입 미도달은 injected=false이며 성공으로 바꾸지 않는다. 합성 IO 오류는
실제 ENOSPC나 호스트 내구성 증명이 아니다. 정상 runtime에 자동 포함하지
않으며 최종 fault build와 해당 descriptor는 별도 검토 대상으로 고정해야 한다.

재현은 기존 fault feature rlib를 --extern nus_exchange_contract로 지정하여
`rustc --edition=2024 --test ops/s3-local/runtime/storage_fault.rs`로 컴파일하고
`--test-threads=1`로 실행한다. 새 설치/lock/component 로직 변경 없음.
