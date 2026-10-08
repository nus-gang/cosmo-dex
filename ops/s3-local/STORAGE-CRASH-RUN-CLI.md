# 저장 crash 인증 실행 CLI

`storage_crash_cli.py check-ready-reviewed`는 START를 보내지 않는다.
`storage_crash_cli.py run-reviewed`는 L-R 최종 pin 승인 후 L-T에서만 실행한다.
기존 reviewed 입력(bundle/artifacts/runtime pin/CEO·CTO revision/native decision,
input-set/scratch/worker 인자)에 다음 옵션을 함께 제공한다:

```
--enable-storage-crash --fault-point before_wal --fault-occurrence 1
--fault-purpose NORMAL --fault-evidence-root /absolute/private/new-report-root
```

두 개발 opt-in도 필수다. 기본 Seal이며 명시적 `--fault-command Apply`도 허용한다. errno/IO fault 옵션 혼합과 Apply/RESOLVE_FAILURE 조합은 거절한다.
인증 stage→동일 capture/C 의미검증→READY→새 audit/바이트/stop 검사→START의
기존 감독 경로를 호출한다. 자식 reap과 private 사본 정리 이후에만 JSON을 출력한다.
exit86은 `outcome=UNKNOWN`, `crash_verified=false`, `replay_verified=false`다.
정상 보고서는 `RECORDED_NOT_REACHED`이며 `command_succeeded`와 별개다.
CLI 종료0은 보고서 수신·검사 성공이며 crash/replay/Seal/DEV 성공이 아니다.
시작 후 오류·중단·정리 실패는 고정 오류와 exit2, stdout0이다.
보고서/home 자동 삭제·수리·재시도는 하지 않는다.

이번 검증: 신규 CLI4 + READY CLI5 + 실행 감독4 = 13 PASS/0 FAIL.
CLI는 mock, 감독은 합성 subprocess. 실제 Rust/C crash 실행·RPC·서비스는 NOT_RUN.
최종 manifest/독립 승인/CTO→Security 심사는 미완료다.


## Apply 내부 연결

`SubmitLane::crash_apply_recorded`는 fault build에서만 제공한다. 승인 Worker의
`Command::Apply`를 한 번 호출하며 C가 영수증 적용·정정을 판단한다. Context,
snapshot ID/hash, 현재 commit, Observation, 시각, selector 및 immediate-exit86을
crash 전용 원문에 결합하고 예약/fsync 후 hook을 설치한다. 반환·오류·panic 뒤
lane을 재사용하지 않는다. 일반 worker는 변경하지 않았으며 CLI의 기본 명령은 Seal이다.

COMMITTED/VOID 영수증이 저장된 fee0/25 fixture에서 미도달 경로를 별도의 새
fixture에 대한 일반 Apply와 대조한다. 기존 home은 inode에 결합되므로 복사·이전해
재생하는 시험으로 대체하지 않는다. 실제 Worker Apply crash/replay는 별도
component 시험에서 검증했고, CLI는 명시적 `--fault-command Apply`로 연결했다.
Apply에는 `--fault-purpose NORMAL`만 허용하며 errno와 crash 옵션을 혼합하지 않는다.
이 component 시험은 runtime 승인 또는 DEV PASS가 아니다.
