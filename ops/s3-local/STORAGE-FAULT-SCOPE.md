# 저장 오류 명령 범위

`runtime/storage_fault.rs::run_command`는 fault-injection C Engine의 기존 set_fault_hook API를 한 명령 주변에서 호출한다. 전용 격리 Engine을 호출자가 독점하고 기존 hook이 없는 상태에서만 사용한다. 일반 worker에는 자동 설치하지 않는다.

선택기 1회 claim → hook 설치 → 명령 1회 → callback 닫힘 → hook 제거 순서다. 명령 오류는 내부 Result, 설치/제거 오류는 바깥 Result이며 오류 후 Engine을 폐기해야 한다. panic은 제거 시도 후 원 panic을 재전파한다. poisoned writer 때문에 제거가 실패해도 남은 callback은 닫히며 해당 Engine은 재사용하지 않는다. SIGKILL/abort는 Rust unwind 정리를 보장하지 않는다.

검증: 신규 scoped lifecycle 4 + 기존 selector 4 = 8 PASS/0 FAIL. setter/command는 주입 경계이며 실제 C API 타입 연결 컴파일을 확인했다. 실제 Engine 저장 오류·replay, worker/CLI 연결, 장애별 DEV 판정은 NOT_RUN이다. 신규 서비스/START/RPC 없음.

두 opt-in, 허용 point/횟수 제한은 유지한다. reported injected는 hook 오류 1회 관측일 뿐 물리 ENOSPC, crash, 내구성 또는 runtime 승인 증거가 아니다.
