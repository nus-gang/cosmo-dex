# 저장 fault 원시 상태 기록

`fault-injection` build에서만 `SubmitLane::fault_seal_recorded`를 명시적으로 호출한다. 일반 scheduler에는 자동 연결하지 않는다. 호출자는 같은 Engine의 단독 소유자여야 하며 실행 후 lane/Engine을 폐기하고 별도 복구 경로를 사용한다.

호출자는 private canonical 절대 증거 디렉터리(0700)와 exact 명령 envelope의 SHA256을 제공한다. SHA는 출처 식별자이고 승인 검사가 아니다. 최종 CLI에서 실제 명령 bytes와 결합해야 한다. 이번 시험의 SHA는 합성 식별자다.

`storage-fault.jsonl`을 0600·no-replace로 예약하고 `reserved` 기록의 file/root fsync를 완료한 뒤 hook 설치와 명령을 실행한다. 종료 시 point/occurrence/visits/matching_visits/injected를 기록한다. 오류 내용·키·명령 원문은 보고서에 직렬화하지 않는다. panic도 기록을 시도한 뒤 그대로 전파한다. 쓰기 실패는 부분 파일을 보존하고 성공으로 반환하지 않는다.

`scope_returned`는 scope 함수의 반환이다. 내부 명령 성공/체인 확정/DEV PASS를 의미하지 않는다. hook 오류 발생 여부는 `injected`이며 미도달과 구분한다. SIGKILL/전원 손실·최종 fsync 오류로 final이 없거나 불완전할 수 있다. UNKNOWN으로 남기고 명령 미실행/내구성 보장으로 해석하지 않는다. 파일을 자동 삭제하거나 같은 명령을 자동 재시도하지 않는다.

이번 검증: 기록 경계 신규3 + 기존 scope/selector8 + 실제 C 명령 기록 연결1, Worker Seal 기록 연결/옵션2. fee0/25 실제 C/L-D/filesystem, 합성 descriptor/pin/키/명령 식별자. 실제 서비스·RPC·방송0. CLI/인증 실행 인계와 final manifest는 아직 미완료다. 기존 G00/ACK와 durable_ack=false 유지.

## F14 Apply 전용 기록 연결

`SubmitLane::correction_apply_recorded`는 실제 Apply 입력의 Context, snapshot ID/SHA,
현재 commit, Observation, 시각, Prepare occurrence와 두 opt-in으로 canonical 명령 원문을 만든다.
`correction-fault.jsonl` 예약을 no-replace/file·root fsync한 뒤 승인 C의 Prepare hook scope를 호출한다.
결과와 관계없이 lane은 닫힌다. `scope_returned`는 내부 Apply 성공이 아니다.
보고서의 주입/phase 방문과 Apply 결과를 따로 확인해야 한다. reader/인증 CLI 연결은 미완료다.
