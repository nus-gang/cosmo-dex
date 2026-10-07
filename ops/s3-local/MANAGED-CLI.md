# 관리 worker CLI — L-R 준비 / L-T 실행

`managed_cli.py serve-reviewed`는 승인 재조회→동일 capture 의미 검증→private worker 사본→READY→새 승인 조회→START→foreground 감독을 연결한다. `launcher_signal.run`의 SIGINT/SIGTERM latch를 사용한다. API credential과 승인 revision은 worker argv/env에 전달하지 않는다. 성공은 출력 없이 exit0, 거절은 고정 진단과 exit2다. 부모 수명은 CLI의 정수 1–300초를 적용한다.

이 파일은 runtime 등록이나 조직 승인이 아니다. 환경 변수만으로 runtime 소유권을 주장하지 않는다. L-R에서는 서비스를 시작하지 않는다. 실제 실행은 최종 exact 후보의 독립 CEO/CTO 출처 및 CTO→Security 심사가 끝난 뒤 L-T에서 Paperclip runtime controls로만 수행한다.

등록할 명령의 형태(아래 경로/UUID/SHA는 완성된 handoff 값으로 대체해야 하며 현재 실행 불가):

```text
<installed-python> -B <candidate>/ops/s3-local/managed_cli.py serve-reviewed
  --bundle <absolute-bundle> --artifacts <absolute-artifacts>
  --input-set <absolute-input-set> --effective-profile <absolute-profile>
  --home <absolute-existing-home> --key-directory <absolute-private-keys>
  --scratch <absolute-private-scratch> --runtime-pin <manifest-sha256>
  --local-demo-profile s3-dev-local/1 --acknowledge-unproven-space
  --bind 127.0.0.1:<rest-port> --rpc 127.0.0.1:<rpc-port>
  --lifetime-seconds 300 --max-requests <bounded-count> --max-ticks <bounded-count>
  --native-decision-id <final-security-decision-uuid>
  --ceo-revision <independent-ceo-revision-uuid>
  --cto-revision <independent-cto-revision-uuid>
```

줄 구분은 설명용이다. 설치된 Paperclip은 shell `-lc`로 command 문자열을 실행한다. `runtime_config.worker_config`의 `shlex.join` 인용 및 `exec` 형식을 사용하며 원 argv를 문자열로 단순 연결하지 않는다. 인증 API reader의 환경 요구는 `APPROVAL-READER.md`를 따른다. API 키를 명령/문서/로그에 넣지 않는다. 자동 restart/retry는 설정하지 않는다.

이번 heartbeat의 인증 GET에서 `currentExecutionWorkspace=null`이었다. runtime 등록·시작0. 최종 launcher 인계 전 실행 workspace 및 서비스 command 설정 경로를 확인해야 한다. 이번 CLI 시험은 run 호출을 mock한 배선/거절 시험4개이며 서비스·실제 Rust 통합 PASS가 아니다. 잘못된 CLI subprocess는 열린 stdin 상태에서도 즉시 exit2다.

남은 범위: 관리 workspace/command 설정, 웹 ChainPort, fee0/25 초기화, chain/web launcher 및 fault/정리, 전체 build/manifest, 독립 승인 출처·네이티브 검토. DEV NOT_RUN·runtime pin 미발급·G00/ACK 유지.

현재 관리 명령/설정은 `--approval-socket` 절대 경로를 필수로 요구한다.
현재 run broker 수명과 인증 전달은 `PRIVATE-READER.md` 후속 절을 따른다.
