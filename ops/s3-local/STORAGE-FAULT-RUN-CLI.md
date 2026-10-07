# 저장 fault 인증 실행 CLI

`storage_fault_cli.py check-ready-reviewed`는 기존 START 없는 검사다.
`storage_fault_cli.py run-reviewed`는 같은 옵션/승인 참조에 대해 전용
fault child를 준비하고 `storage_fault_run.run`을 한 번 호출한다.
실제 실행은 승인 runtime pin을 인수한 L-T에서만 수행한다.

명령 형태: `python3 -B ops/s3-local/storage_fault_cli.py run-reviewed`
뒤에 기존 `check-ready-reviewed`의 입력 bundle/artifacts/runtime pin,
input-set/worker home/signer/RPC 및 CEO/CTO revision·native decision 옵션,
두 opt-in을 동일하게 지정한다. 추가 필수 옵션은
`--enable-storage-fault --fault-point before_wal --fault-occurrence 1
--fault-purpose NORMAL --fault-evidence-root /absolute/new-private-evidence`다.
정확한 공통 인자는 `offline_cli.parse(reviewed=True)`를 재사용한다.
별도 lifetime/START 인자를 허용하지 않으며 READY 5초·실행60초 상한이다.

현재 run 인증 reader와 stage의 C 의미검증, READY 뒤 audit,
START 직전 새 audit/바이트/stop 검사를 유지한다. signal scope는
준비부터 child 종료와 private 사본 정리까지 포함한다. 최종 JSON은
모든 scope 종료 후에만 출력한다. command_succeeded와 injected는
독립 bool이며 child_exit=0이 Seal 성공 또는 replay 성공을 뜻하지 않는다.
오류/interrupt/정리 실패는 stdout0·exit2와 고정
`LOCAL_STORAGE_FAULT_RUN_REJECTED_OUTCOME_UNKNOWN`을 반환한다.
실행 전 거절에도 보수적으로 같은 진단을 사용하므로 진단만으로
명령 수행 여부를 추정하지 않는다. 재시도·보고서/home 삭제는 없다.

이번 검증: 신규 CLI4 + 기존 READY CLI4 + 실행 감독4 = 12 PASS.
CLI stage/audit/run은 mock, 실행 감독은 합성 subprocess다.
실제 Rust/C 실행 연결과 서비스/RPC/START는 이번 NOT_RUN이다.
최종 descriptor/manifest·독립 승인·CTO→Security 심사가 남아 있다.

## Apply 명시 선택

기존 옵션에 `--fault-command Apply`를 추가하면 전용 child의
`fault-apply-captured` → 단일 신뢰 관측 → 승인 Worker Apply 저장 fault를 호출한다.
생략/Seal은 기존 Seal 경로다. 공통 옵션의 `--fault-purpose NORMAL`은 Apply에서는
호환용 고정값이며 RESOLVE_FAILURE와 조합하면 조회 전에 거절한다.
Apply 결과 schema는 `s3-local-fault-apply-result/1`이며 Seal 응답과 상호 거절한다.
Generic/v2 및 ENOSPC/EDQUOT/EIO/v3 보고서 규칙은 유지한다.
관측 저장은 fault 보고서 예약보다 먼저다. START 이후 오류는 결과 불명이며
재시도/증거 삭제를 하지 않는다. 실제 실행은 승인 후보의 L-T 범위다.

이번 검증: Python 배선/합성 child, 컴파일, 실제 child 잘못된 입력 거절.
유효 Apply child READY 및 실제 RPC/START/Apply 종단은 NOT_RUN.
