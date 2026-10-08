# Paperclip 관리 worker 설정 준비

2026-10-07, NUS-73 L-R. `runtime_config.worker_config`는 순수 설정 생성기다.
API 호출·등록·서비스 시작·승인 판정을 하지 않는다. 최종 manifest/pin 미발급,
DEV NOT_RUN. 합성 config를 운영/시연 pin으로 사용하지 않는다.

## 확인된 연결 경로

인증 GET의 project workspace `61a53d01-c9ea-45dd-b274-eb954095ee6a`는
`runtimeConfig=null`, `runtimeServices=[]`다. 이전 heartbeat의 execution
workspace=null과 별개로 다음 프로젝트 workspace API가 OpenAPI에 존재한다.

- 설정: `PATCH /api/projects/{id}/workspaces/{workspaceId}`, `runtimeConfig`
- 명시 시작/종료: `POST /api/projects/{id}/workspaces/{workspaceId}/runtime-services/{start|stop}`,
  body `{"workspaceCommandId":"s3-worker-fee0"}` (별도 fee25)

공유 primary workspace 설정은 변경하지 않았다. 최종 L-T 인계는 기존
workspace의 서비스를 덮어쓰지 않는 전용 workspace를 선택해야 한다.
설정·시작 API의 실제 성공 및 포트 해제 시험은 아직 수행하지 않았다.

## 실제 command 형식

설치된 `@paperclipai/shared/dist/workspace-commands.js`는
`workspaceRuntime.commands[].command` 문자열을 받는다. server의
`workspace-runtime.js`는 shell `-lc`로 실행한다. 따라서 argv 단순 연결은
금지한다. 생성기는 `exec ` + `shlex.join(argv)`를 사용하며 제어문자 및
템플릿 구문을 거절한다. `/bin/sh`, `/bin/zsh`의 실제 비서비스 subprocess로
따옴표·공백·세미콜론·명령 치환·환경 변수 문자가 원 인자로 유지됨을 검증했다.

`--rpc`는 Rust `startup.rs`의 SocketAddr 입력이므로 `127.0.0.1:26657`처럼
스킴 없이 쓴다. 기존 MANAGED-CLI.md의 http:// 표기는 정정했다.

`desiredState=manual`, `serviceStates={"0":"manual"}`을 사용한다. 설치된
server의 `buildWorkspaceRuntimeDesiredStatePatch`는 명시적 start에도 manual을
보존한다. 자동 restart/retry를 요청하지 않는다. 노출·proxy·설치·env 항목은
생성하지 않는다. fee0/25 각각 독립 설정이며 fee 번호는 이름일 뿐 실제
경제 설정 증명이 아니다. Context/profile/home 의미 검증은 기존 validator다.

## 남은 인증 연결

설치된 runtime은 기본 환경의 `PAPERCLIP_*`를 제거하고 프로젝트 제어 route는
`adapterEnv={}`를 전달한다. 현재 Reader.from_environment에 필요한 API 인증이
자동 상속된다고 가정할 수 없다. 토큰을 command/JSON/env 설정에 저장하거나
승인 검사를 건너뛰지 않는다. SRE가 L-T 현재 run의 인증을 유한 수명의 private
IPC 등으로 전달하는 경계를 구현·검증한 뒤 최종 설정에 결합해야 한다.
현재 생성 결과만 등록하면 인증 누락으로 fail-closed될 수 있으며 실행 가능
최종 인계가 아니다. 승인 검사는 각 시작에서 다시 수행해야 한다.

## 검증 근거와 다음 단계

- 순수시험3 PASS. 최초 RPC 형식 오류1은 수정 후 재시험했으며 로그 보존.
- 설치된 shared parser의 실제 config 읽기 PASS. 서비스 spawn0.
- 설치 source3개 SHA256를 증거 JSON에 보존. OpenAPI는 인증 GET으로 조회.
- 다음 SRE 작업: private 인증 전달·managed launcher 결합, 웹 ChainPort,
  fee0/25 초기화·chain/web/fault/정리, 최종 build/manifest, 독립 CEO/CTO 출처와
  CTO→Security 심사. 실제 서비스는 L-T. G00/ACK·표준 부모 blocker 유지.

현재 관리 명령/설정은 `--approval-socket` 절대 경로를 필수로 요구한다.
현재 run broker 수명과 인증 전달은 `PRIVATE-READER.md` 후속 절을 따른다.

## 설치 권한 경계 정정

후속 조사에서 host command POST/PATCH의 agent 금지 정책을 확인했다.
전용 workspace 등록 자료와 대조 함수는 `workspace_registration.py`, 근거와
아직 남은 고정 명령/동적 IPC 문제는 `WORKSPACE-REGISTRATION.md`를 따른다.
현재 생성기 결과만으로 agent가 등록/기동할 수 있다고 해석하지 않는다.
