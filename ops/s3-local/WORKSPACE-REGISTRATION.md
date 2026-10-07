# 전용 workspace 등록 준비 및 설치 권한 경계

2026-10-07 · NUS-73 L-R. runtime pin 미발급, DEV NOT_RUN.

`workspace_registration.prepare`는 fee0/25 각각 새 local_path workspace의 POST
요청 자료를 만든다. `isPrimary=false`, manual, 명시적인 단일 worker command,
현재 run private socket을 포함한다. API를 호출하지 않으며 기존 workspace를
PATCH하지 않는다. 이 자료는 조직 승인이나 실행 허가가 아니다.

`registered`는 준비한 자료와 새 인증 GET 목록의 동일 이름 1개를 대조한다.
회사/프로젝트/cwd/command/config가 같고 primary·shared·remote·setup/cleanup·
기존 runtime이 없어야 한다. 실패한/stopped service 기록도 현재는 거절한다.
검증 결과는 특정 workspaceCommandId의 start/stop 요청 자료일 뿐이며 실제
전송·재시도·승인·서비스 상태 감시는 구현하지 않았다. GET/start 사이 경쟁을
제거하지 않는다. 기존 의미 검증 및 시작 직전 audit도 계속 필요하다.

## 실제 설치에서 확인한 제약

설치 버전 경로 `2026.916.1`의 `server/dist/routes/projects.js`는 workspace
POST/PATCH에 `assertNoAgentHostWorkspaceCommandMutation`을 호출한다.
`workspace-command-authz.js`는 agent actor가 host command를 포함하면
`Agent keys cannot modify host-executed workspace commands`로 거절한다.
설치 모듈에 합성 요청을 전달해 schema 수용과 agent 거절을 검증했다.
실제 write 요청·권한 변경·우회는 수행하지 않았다.

현재 인증 GET은 workspace 1개, `isPrimary=true`, `cwd=null`,
`runtimeConfig=null`, `runtimeServices=[]`를 반환했다. 이 workspace를
자동 실행 대상으로 사용하거나 덮어쓰지 않는다. 새 workspace 등록은
권한 있는 board 경로가 필요하다. 현재 run마다 바뀌는 private IPC endpoint를
명령에 저장하면 다음 run에서 재등록이 필요하다. 따라서 이전 안내의
“L-T가 config를 등록하면 실행 가능”은 아직 완성된 실행 경로가 아니다.

SRE는 고정된 등록 명령과 현재 run 인증 IPC를 연결하는 설계를 완성하고,
실제 worker/chain/web·초기화/fault/정리·최종 manifest와 심사 자료를 준비한
후 구체적인 등록 결과물을 권한 있는 경로로 넘겨야 한다. 미완성 자료에
대한 사용자 확인 요청은 이번에 만들지 않았다. agent actor 변조, raw metadata
우회, DB 쓰기, 관리되지 않는 서비스 실행으로 이 제한을 피하지 않는다.

검증: 순수시험3 PASS/0 FAIL. fee0/25 요청, 대조 결과 복사, primary/타 회사/
타 프로젝트/명령·환경·cwd 변경/중복/누락/기존 service 거절.
설치 schema/권한 함수 대조 PASS (위 3개와 시험 수 합산하지 않음).
최초 sandbox의 localhost GET 연결 실패는 허용된 확대 read-only 재실행으로
해결했다. 실제 runtime 등록/시작/Chain RPC0, 비용 €0.

원 G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED 및 표준 부모 blocker 유지.
CTO→Security 심사 제출 전이며 독립 CEO/CTO runtime 승인 출처는 미발급이다.

## 고정 broker 경로 연결 (2026-10-07 후속)

`private_reader.broker_at(root)`는 기존 `--approval-socket <root>/s` 명령을
변경하지 않고 매 L-T run의 인증 Reader를 공급한다. 전용 parent는 canonical
경로·현재 uid·0700이어야 하며 Mac socket 경로는 103 bytes 이하다. root를
mkdir(no-replace)로 독점한다. 파일/디렉터리/링크가 이미 있으면 stale 여부와
무관하게 거절한다. crash 잔여 경로는 자동 삭제하지 않는다.

사용 순서: 권한 있는 경로에서 exact 고정 command를 등록한 뒤 L-T가
`with broker_at(고정_root) as endpoint:` 안에서 관리 runtime start/stop을
수행해야 한다. 현재는 broker API만 연결했고 그 orchestration은 남아 있다.
등록 command·manifest·승인 후보·서비스 인자를 바꾸는 동적 우회가 아니다.
토큰은 현재 run 메모리에만 있고 IPC에는 제한된 fresh GET 결과만 흐른다.
종료 시 thread join 뒤 socket과 자신이 만든 root만 제거한다. 예상 밖 파일은
보존하고 cleanup 오류로 닫는다. 같은 uid의 악성 프로세스는 기존 신뢰 경계다.

신규3+IPC 회귀5 = 8 PASS/0 FAIL. fixed 경로 두 번 사용·fresh reader·중복
broker 거절·stale 파일/링크 보존·private parent·socket 생성 실패·interrupt·
추가 증거 보존을 검증했다. 최초 실행 cwd/로그 경로 오류, sandbox bind 거절,
Mac 경로 길이 fixture 오류를 보정했고 로그를 보존했다. API 등록/start0,
실제 Rust worker 재시험0, 서비스/Chain RPC0. runtime 승인 미발급이다.

## 관리 session 내부 조합 (2026-10-07 후속)

`managed_session._session`은 아직 HTTP client나 공개 실행 CLI가 없는 내부
조합 함수다. trusted bounded broker/read/request/audit adapter를 주입한다.
등록 자료를 원 입력에서 다시 생성하고 고정 socket 일치를 먼저 검사한다.
broker 안에서 fresh workspace 조회→audit→workspace 재조회→대상 command
start→호출자 작업→같은 command stop 순서다. command/config 변경은 start0,
start 응답 유실·작업 오류·KeyboardInterrupt도 stop을 정확히 한 번 시도한다.
broker는 stop 시도 뒤 닫는다. 불명 start의 자동 재시도는 없다.

stop IO 실패는 `MANAGED_SESSION_STOP_UNCONFIRMED`이고 성공으로 숨기지 않는다.
성공 응답도 `process_exit_verified=false`이다. 실제 설치 API 응답 검증,
종료 service 상태/프로세스 확인과 결과 보존을 연결하기 전 정리 완료라고
표시하지 않는다. GET/start 간 경쟁을 완전히 제거하지 않으며 worker의
READY 이후 독립 승인·실행 바이트 재검사를 유지한다. 함수의 audit callback은
테스트용 trusted seam이지 설정에서 받아들이는 승인 허가가 아니다.

신규 합성 시험5+기존 등록 회귀3 = 8 PASS/0 FAIL. fee0/25 순서, 승인 거절,
config 변경, 잘못된 socket, 시작 응답 유실, 작업 실패/interrupt, stop 실패와
재시도0을 검증했다. 최초 Path/string 비교 오류를 수정하고 실패 로그 보존.
실제 관리 API write/IPC/서비스/Chain RPC0, 실제 Rust/C 재시험0.
남은 연결: 인증된 제한 API transport·실제 응답/종료 증거·L-T orchestration,
웹 ChainPort·초기화/chain/web/fault/정리·최종 manifest·독립 승인.

### 관리 operation과 종료 상태 대조

`managed_session._observed_session(..., client=RuntimeClient, ...)`는 내부 조합
API다. start/stop HTTP 200 뒤 operation `succeeded`, action/phase, 회사·프로젝트·
workspace·command·cwd와 완료 시각을 검사한다. start 오류/응답 유실도 기존
session의 대상 stop 1회 경로로 들어간다. stop 응답 뒤 인증 workspace GET을
1회 수행하고 같은 exact 서비스 행·config의 stopped 상태를 대조한다.
누락 행·재시작·변조·노출·실패·조회 오류는 종료 확인 실패이며 재시도하지 않는다.

`control_plane_stop_verified`는 시점 한정 제어면 관측이다.
`process_exit_verified`, `port_release_verified`, `writer_release_verified`는
false로 남긴다. L-T에서 프로세스/포트/같은 home writer 재개방의 실제 증거를
추가해야 한다. 이 API는 실행 CLI 또는 runtime 승인·DEV PASS가 아니다.

### L-T 포트 해제 관측 준비

`port_release.check(endpoints)`는 승인 manifest/runtime 설정에서 얻은 명시적
`[("127.0.0.1", port), ("::1", port)]` 목록을 검사한다. 최대 32개, 정수
1024–65535, 중복 없는 loopback TCP만 허용한다. 포트별 socket을 모두 확보한
동안 `bind` 성공을 확인하고 역순으로 닫는다. `listen/connect` 및
`SO_REUSEADDR/SO_REUSEPORT`는 사용하지 않는다. IPv6는 V6ONLY로 제한한다.
실패·중단 시 이미 만든 socket을 닫으며 재시도하지 않는다.

보고서는 `simultaneous_bind_verified`와 monotonic 관측 구간을 남긴다.
점검 이후 포트 예약, 기존 process 종료, writer 해제는 입증하지 않는다.
OS별 reuse 규칙 때문에 bind 결과만으로 모든 기존 socket 부재를 주장하지
않는다. 관리 API `stopped` 증거와 별도로 보존한다. 현재 session 자동 연결과
실제 호스트 bind 시험은 미실행이며 L-T에서 승인된 후보·종료 절차에 연결한다.
L-R 신규 4개 시험은 socket/clock 주입의 순수시험이다.
