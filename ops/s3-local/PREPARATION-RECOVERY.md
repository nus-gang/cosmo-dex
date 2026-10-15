# RC9 준비·실행 시간과 L-T-RC8-01 보존

NUS-73 CTO 판정 `7d828e6d-4f46-477d-9a83-aa1f1469555b`의 수정 후보다. 서비스 실행은 새 manifest의 CTO→Security 및 NUS-75의 새 CEO/CTO canonical 승인·composed gate 이후 NUS-74에서만 수행한다. 과거 RC8 승인·실패 자료·두 profile home·guard·키·ledger는 보존한다. 기존 탭의 키가 소실된 home/등록 packet12개는 거래에 재사용하지 않는다.

## 준비와 유지 시간

준비/최종 웹 CLI는 인자·두 opt-in·신호 handler 확인 뒤 첫 승인 IO **전에** 자기 PID를 기존 nonce-bound `pids.json`으로 게시한다. PID는 process handoff이며 HTTP ready가 아니다. 승인·실제 bytes·마지막 승인·stop 검사 후에만 loopback bind/listen한다. 첫/마지막 audit, assets, bind, http_listening, closed 또는 failed를 별도 `preparation-<nonce>/NN.json`에 남긴다. 실패 분류는 operator_stop/approval/files/port/transport/internal의 고정 코드다. 예외 원문·인증 토큰·키는 기록하지 않는다. 파일 실패 때문에 단계 자체가 누락될 수 있으며 누락은 성공으로 해석하지 않는다. 이벤트는 fsync된 pending 파일의 마지막 no-replace rename으로 공개하며, 부분/pending은 ready가 아니다.

`restart_session_cli.py run-reentry-reviewed`는 다음 순서다.

1. 이전 정상/실패 이력 대조 → 두 전체 approval audit → 최종 자원 관측 → final GET → 최신 권한 재조회 → confirm → 관리 START. 기존 60초 자원 관측/2초 최종 freshness와 일회성 객체를 유지한다.
2. 관리 START 응답 이후 별도 **60초 이내** nonce/PID/단계와 HTTP `GET /`의 200·실제 HTML SHA를 확인한다. HTTP는 직접 loopback·정확한 Host·전체 IO 최대1초·최대80KiB·redirect/proxy/retry0이다. PID만 있거나 준비 로그만 있어도 유지 시간을 시작하지 않는다. stage 대기는 bounded 관측이며 START를 재시도하지 않는다.
3. `session-<nonce>/readiness.json`이 생긴 시점부터 요청의 `duration_seconds`를 센다. 다음 정상 L-T는 **120**을 사용한다. 준비 시간은 이120초에 포함되지 않는다. 준비 timeout/stop/오류는 대상 STOP1회를 거치며 `session-<nonce>/outcome.json`과 원 mailbox를 보존한다. 정상 archive 생성은 기존 성공적 context 종료·STOP/목록/PID/자원 조건을 모두 통과한 경우뿐이다. 실제 운영자 stop으로 유지 시간이 짧아진 것은120초 완료가 아니다.
4. 정상 STOP·fresh listing·PID/port 검사와 v2 archive 공개는 기존 경로다. 웹 자체 serve 수명300초·요청100개(준비)/기존 명시 상한(최종)을 유지한다. chain/worker의 기존 실행·writer 경계는 변경하지 않는다. 관리 API 자체의 기존 bounded IO는60초 HTTP 준비 대기와 별도다.

## 이미 발생한 challenge-only 사건

초기 PID 수정을 과거에 소급하지 않는다. `fixtures/failed-preparation-rc8/incident.json`은 CTO ZIP `e7dfe2ad…47fbf`와 SRE ZIP `1a564b78…702a`의 공개 원문을 묶은 **이 사건 한정** 입력이다. SHA256 `2b64ebc89b3a0808f373f2e4f196aadd5c5cea87e750cb0979ebd9061c8cc2e5`가 코드에 고정돼 있다. 임의 실패·다른 nonce/row/command/config·조작한 성공 flag는 거절한다.

이 자료는 START `a62e966b-01ae-4dde-8bd4-2991d4048f39`, STOP `847fae98-5bd8-43ca-b641-743a5616f9d7`의 원 operation 로그·정확한 packet·service3행·이전 정상 receipt3개·원 challenge·당시 PID/port 관측과 출처 revision을 보존한다. 공개 operation 로그에는 run ID와 전체 원 POST 응답이 없으므로 그것을 재구성하지 않는다. `operation_run_binding=UNAVAILABLE_IN_PUBLIC_OPERATION_LOG`, `pid_handoff=MISSING`, `failure_branch=UNPROVEN`, `http_readiness=UNPROVEN`, `normal_receipt=false`, `cleanup_complete_verified=false`다. 이 제한까지 포함한 **새 후보의 독립 검토**가 필요하며 이전 CTO 조사만을 실행 허가로 사용하지 않는다.

새로운 private 보존 부모에서 아래 명령으로 복사한다. 명령은 서비스/등록/승인을 실행하지 않는다. 목적지 basename은 `failed-<원 challenge64자>`여야 한다.

```sh
python3 -B <새_candidate>/ops/s3-local/restart_session_cli.py \
  preserve-reviewed-preparation-failure --destination <새_보존_부모>/failed-<원_nonce>
```

원 mailbox에 challenge만 있고 해시가 일치하며 pending이 없어야 한다. 원본 receipt3개도 실제 archive와 대조한다. 새 archive에는 `failure.json`, `incident.json`, `challenge`만 복사한다. 실패/충돌 시 원본과 pending을 보존하며 자동 rm/mv/수리/재시도하지 않는다. 최종 no-replace rename으로 공개하고 정상 `receipt.json` 또는 `pids.json`을 만들지 않는다. 전원 장애 시 최종 이름의 지속성은 여전히 미입증이다.

다음 요청은 `schema: s3-local-reentry-session/2`, `failed_preparations: [<failure.json 절대경로>]`를 추가한다. `receipts`는 현재 각 service의 기반인 기존 정상3개를 모두 유지한다. 이 실패 자료는 그중 재사용된 ID `89cf8ef6-e801-48b4-bc81-95da712047f5`의 **변경된 최신 행**만 별도 사실로 결합한다. 다른 두 행과 모든 packet/nonce/STOP/PID fields는 그대로 비교한다. 명시적 import로 정상 archive를 복사한 경우 archive_root만 달라질 수 있다. 빈 receipts, 다른 행 숨김, 같은 pin/경로로 재개, duplicate workspace는 거절한다.

새 candidate의 keys→keys 전환이 반드시 있어야 한다. RC4→RC6→RC8→RC9 준비를3개 edge로, RC9 준비→최종 웹까지 최대4개 연속 edge로 보존한다. 4개 허용은 이 실패 이력이 있는 경우에만 적용한다. 실제 source/cwd/bundle/artifacts/pin/final decision/CEO·CTO revision/IPC/mailbox가 모두 새 값이어야 한다. 모든 등록·최종 confirm에서 실패 archive 및 **원 failed mailbox**도 다시 읽는다.

기존 정상 receipt의 자원 관측을 유지하고, 실패 행의 제어 plane PID15632/47145 부재와5173 가용성을 매번 별도로 확인한다. providerRef를 child handoff로 바꾸지 않는다. PID 재사용·생존·권한/조회 오류·원 파일 변조·더 최신 service 실행이 있으면 거절한다. 이 관측은 전체 자손 종료·지속 포트 예약을 입증하지 않는다. 같은 uid와 인증된 control plane의 provenance를 신뢰하는 로컬 범위이며 일반 장애 복구 허가는 아니다.

## 보드 설정 전환과 탭 유지

설치 `projects.js`의 기존 `PATCH /api/projects/{projectId}/workspaces/{workspaceId}`는 `assertNoAgentHostWorkspaceCommandMutation`을 적용한다. 권한 있는 보드 세션의 기존 UI/API가 실제 설정 경로이며 SRE agent token으로 우회하지 않는다. 새 승인 전에는 이 변경을 실행하거나 미완성 packet을 사용자에게 요청하지 않는다.

1. 새 pin/composed gate 후 새 control root와 새 준비 packet을 완성한다. 기존 workspace ID를 유지한 채 `cwd`와 `runtimeConfig`를 packet 그대로 갱신하는 구체 PATCH 파일·원문 SHA·현재 설정 diff를 만든다. 원 service 행 삭제·workspace 추가·예전 RC8 설정 재적용은 하지 않는다. 보드가 실행 가능한 경로와 담당을 확인한 뒤 정확한 새 packet만 인계한다. 응답 유실은 GET 대조하며 자동 반복하지 않는다.
2. 설정 GET을 exact packet과 대조하고 위 실패 이력+정상3개+연속 전환으로120초 준비 웹을 실행한다. **readiness.json 이후** 브라우저에서 두 opt-in과 준비 버튼을 사용해 profile별 새 두 사용자의 공개 등록 데이터를 만든다. 탭은 열린 상태로 유지하고 공개 데이터만 initializer에 전달한다. 개인키를 export하지 않는다.
3. 새 Context/genesis/home을 만들고 B/C 초기화 검증을 마친 뒤 initializer 보고서로 정확한 최종12 packet을 생성한다. 모든 필요한 chain/worker 설정과 해당 profile의 최종 웹 설정을 구체 파일로 준비한다. 보드 경로·실행 담당·변경할 기존 ID와 응답 대조 방법을 확인한다. 탭 키가 있는 동안 처리할 수 없는 조건이면 거래를 시작하지 않고 실패/탭 종료 사실을 보존한다.
4. 준비 웹120초 종료와 STOP/release/v2 archive를 확인해도 **브라우저 탭을 닫거나 reload/navigation하지 않는다**. 보드가 같은 workspace에 최종 packet의 cwd/runtimeConfig를 반영한다. GET에서 실제 변경을 확인한 다음 같은 후보 승인·새 nonce로 최종 서비스들을 관리 runtime에서 실행한다. 계정 탭은 동일 origin을 유지하고 기존 `activate` 버튼으로 `/runtime-context.json`을 조회해 연결한다. `pagehide`는 키를 파기하므로 새 페이지로 이동해 복구하는 절차는 없다.
5. fee0/25는5173을 공유하므로 순차 실행한다. 각 profile 탭/Context를 구분하고 이전 profile의 STOP/release 이후 전환한다. 거래/정산/출금과 DEV/AR은 NUS-74의 새 실제 관측으로만 판정한다.

SEC-RC6-R1/R2, RC8 latency, 원 G00=FAIL_UNPROVEN/allowlist=[]/ACK=CLOSED/durable_ack=false, 운영·외부 Linux 미검증과 비용€0를 유지한다.
