# 등록 packet CLI

L-R 준비 명령이며 API 등록/서비스 시작/mailbox 생성은 하지 않는다.

```sh
python3 -B ops/s3-local/registration_cli.py packet \
  --python /absolute/python3 --candidate /absolute/candidate \
  --fee-bps 0 -- <managed_cli.py의 serve-reviewed 이후 전체 인자>
```

fee25는 `--fee-bps 25`로 별도 생성한다. `--` 뒤 인자는 기존
MANAGED-CLI.md의 두 opt-in, exact pin/판정/revision, private socket/mailbox,
Context/home/signer, loopback 및 실행 상한을 모두 포함해야 한다.
대괄호 부분은 설명용이며 실제 명령에는 검토한 인자를 직접 나열한다.
명령은 결정적인 JSON 1개를 stdout으로 출력한다. 잘못된 입력은 exit2,
stdout0, 고정 오류만 남긴다. 인자 축약/중복/추가 command는 허용하지 않는다.
출력은 비밀 키/토큰을 포함하지 않지만 로컬 경로를 포함하므로 내부 검토 자료다.

packet의 `requires_board_registration=true`, `approval_verified=false`,
`starts_service=false`를 유지한다. 설치 Paperclip의 host command 등록 제한은
변경하지 않는다. 최종 exact 후보의 권한 있는 등록 경로에 전달할 자료이며,
현재 문서의 예시가 등록 완료/조직 승인/runtime pin 발급을 의미하지 않는다.
등록 작업은 최종 후보가 준비된 뒤 구체 packet으로 인계한다.

이번 검증: 신규 CLI4 + 기존 runtime_config3 = 7 PASS/0 FAIL.
순수 설정/합성 인자 및 거절 subprocess 시험이다. 실제 Rust/C 재시험0,
API 등록/시작0. 최초 시험 파일 SyntaxError를 보정 후 통과했다.

인증 session의 최종 실행 CLI, 웹 ChainPort, 새 genesis/home,
chain/web launcher와 fault driver, 최종 manifest 및 독립 심사는 남아 있다.
DEV NOT_RUN, runtime pin 미발급, G00/ACK/표준 부모 blocker 유지.
