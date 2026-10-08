# F14 인증 READY 검사·실행

`python3 -B ops/s3-local/correction_cli.py check-ready-reviewed`에 기존
`offline_cli.py`의 reviewed 필수 인자와 다음 인자를 전달한다.

```text
--enable-f14-prepare
--fault-occurrence 1
--fault-evidence-root /absolute/private/new-evidence
```

`--local-demo-profile s3-dev-local/1`, `--acknowledge-unproven-space`와 exact
runtime pin, native decision ID, CEO/CTO revision이 필요하다. 현재 run의 인증
reader로 승인 출처를 읽는다. 전용 F14 descriptor/private 사본과 같은 capture의
C validator를 거쳐 READY 이후 승인을 다시 검사한다.

이 명령은 START를 보내지 않는다. child reap 및 private 사본 정리가 끝난 뒤에만
JSON을 출력한다. 성공에도 `F14_verified`, `fault_started`, `service_started`,
`approval_verified`, `reusable_permit`은 false다. errno/phase/다른 fault command,
중복·축약·잘못된 경로·비정규 occurrence는 거절한다. 실패는 exit2, stdout0,
고정 `LOCAL_F14_CHECK_REJECTED`이며 민감한 예외를 출력하지 않는다.

이번 CLI 시험은 mock stage/audit/READY와 실제 거절 subprocess다. 별도 READY/stage
회귀는 합성 child/bytes를 사용한다. 실제 Rust/C 연결의 기존 근거와 구분하며
실제 F14 child의 START 직전 거절 연결·최종 manifest·독립 심사는 남아 있다.


승인된 L-T 실행에서는 같은 인자로 `run-reviewed`를 사용한다. 세 번째 승인
조회와 binary/capture·stop 검사를 통과한 뒤 기존 감독이 START+EOF를 보낸다.
관측 저장은 fault 보고서 예약보다 앞선다. 명령 결과의 `command_succeeded`,
`injected`, Prepare/SemanticReplay 방문 수를 그대로 구분하며 F14·replay·조직
승인 인증은 false다. 정상 보고서도 Apply 성공을 자동 의미하지 않는다.
child reap·private 사본 정리 이후에만 stdout JSON을 내며, 오류/중단/정리 실패는
exit2/stdout0과 `LOCAL_F14_RUN_REJECTED_OUTCOME_UNKNOWN`으로 끝난다.
증거/home을 삭제하거나 재시도하지 않는다. 이번 시험은 mock CLI와 합성 child
감독이며 실제 서비스/START/RPC 또는 F14 실행 결과가 아니다.
