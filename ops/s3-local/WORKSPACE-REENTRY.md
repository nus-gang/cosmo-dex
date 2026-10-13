# 종료한 관리 workspace의 재시작과 웹 전환

L-T-RC4-01 수정. `runtimeServices` 원문을 유지한다. `registered()`의 기본값은 여전히 새 빈 workspace만 받으며, 종료 이력은 `Reentry`와 실제 STOP/PID/nonce 근거가 있어야 인수한다. 이 변경의 사용은 새 manifest의 CTO→Security·CEO/CTO 독립 승인 이후 L-T에 한정된다. RC4 pin과 RC4 증거용 fee0 home은 새 실행에 사용하지 않는다.

`restart_session_cli.py run-reentry-reviewed --request /절대경로/request.json`은 이미 등록된 정확한 command를 한 번 START하고 반드시 대상 STOP을 시도한다. 새 등록이나 설정 변경 API를 호출하지 않는다. request는 다음 필드만 가진다.

```json
{
  "schema": "s3-local-reentry-session/1",
  "packet": "등록 생성기가 낸 실제 packet 객체로 대체",
  "workspace_id": "실제 등록 workspace UUID",
  "duration_seconds": 120,
  "receipts": [],
  "transition": null
}
```

- `packet`: `key_packets(spec)` 또는 `registration_render.render(spec,reports)`의 정확한 객체. 문자열 예시는 실행 입력이 아니다. 준비 웹/최종 웹/worker/Chain 네 종류를 동일 CLI에서 선택한다. Python/argv/cwd/port/manual config를 기존 생성기로 재구성해 일치시킨다.
- 최초 실행은 receipts=[]이다. 정상 STOP·fresh listing·PID/port(Chain/worker는 writer까지) 확인 후 `completed-<nonce>/receipt.json`과 원 mailbox를 no-replace/fsync 경계로 보존하고 경로를 출력한다. 다음 호출은 같은 mailbox 경로에 새 nonce를 만든다. 중단/START 응답 유실/STOP 불명/정리 실패는 자동 재시도·삭제·nonce 교체 없이 원 mailbox를 보존한다.
- 재시작은 `receipts`에 현재 listing의 **각 service ID마다 최신 정상 종료 receipt의 절대 경로**를 넣는다(최대32). 같은 ID의 이전 receipt는 파일로 보존하지만 최신 증거와 중복 입력하지 않는다. 다른 command의 이전 stopped 행도 모두 목록에 있어야 한다. 재사용되거나 새로 생긴 service ID는 다음 STOP 원문에서 확인한다.
- 준비→최종 웹은 `transition={"previous": <정확한 준비 packet>, "next": <정확한 최종 packet>}`를 추가한다. 두 packet의 workspace명/cwd/Python·pin·승인 revision·IPC·origin이 같아야 하며, 새 최종 packet이 fresh 인증 listing의 현재 config와 같아야 한다. 필요한 board의 권한 있는 config 변경은 기존 등록 workspace에서 수행한다. 이 CLI는 설정을 바꾸거나 등록 제한을 우회하지 않는다. 임의 command·다른 candidate로의 암묵 전환은 거절한다.
- 두 등록 조회에서 receipt 원문/실제 archive와 company/project/workspace/scope/command/STOP service/PID를 대조하고 자원 해제를 새로 관측한다. 기존 관측 성공 flag는 허가로 쓰지 않는다. 순서는 등록/자원1 → fresh 승인 audit1 → fresh 승인 audit2 → 등록/자원2 → final GET → 인증 권한 재조회 → confirm → START다. 마지막 자원 검사 종료부터 confirm까지 2초 상한을 유지한다. IO/시계/시간상한/변경은 해당 인수 객체를 닫는다.
- worker 재진입은 pinned capture와 실제 offline validator로 같은 home을 open/drop한다. Chain은 기존 `writer.dev.lock`을 생성·삭제 없이 다시 획득/해제한다. 원 home/guard/genesis를 재생성하거나 수리하지 않는다.

API 직접 조합은 `Reentry(packet, receipt_paths, transition=...)`를 authenticated web/Chain/worker session의 `reentry=`로 전달하고, 성공적으로 context를 나온 뒤 `preserve(mailbox, packet, evidence)`로 보존한다. 실패한 context에서 preserve를 호출하지 않는다. 새 CLI는 이 순서를 구현한다. 기존 일회 CLI도 보존되며 종료 이력에 대한 암묵 fallback은 없다.

동일 uid와 인증 API reader를 신뢰하는 로컬 경계다. PID 목록·관측 시점의 포트/lock 해제는 모든 자손 종료나 지속 예약을 보장하지 않는다. API GET과 START 사이의 원자적 compare-and-start는 설치 API가 제공하지 않는다. 세 번째 조회 뒤의 경쟁까지 없앴다고 주장하지 않으며, managed child의 독립 승인/byte/READY gate와 단일 writer가 그대로 남는다. 실제 준비→최종 웹 및 같은-home 서비스 재시작의 DEV/AR 판정은 새 승인 후 NUS-74에서 수행한다.

`fixtures/stopped-workspace-rc4`는 실제 RC4 L-T 원문과 공개 nonce/PID 증거다. 출처: NUS-74 attachment `91a2ee9d-afa0-4ddd-b23f-17ea2a027e2f`, CTO 재현 `4d3ea526-dff6-4845-af68-6f1b0db7f7c1`. 시험은 원문을 임시 archive로 복사해 사용하며 원 서비스나 home을 실행/변경하지 않는다. `test_workspace_reentry.py`는 원문 결합·거절·보존, `test_reentry_sessions.py`는 합성 control plane 및 실제 mailbox 파일을 사용하는 caller/CLI 회귀다. 후자의 합성 START는 제품 서비스 실행이 아니다.

RC4 준비 workspace를 새 후보 준비 workspace로 갱신할 때도 같은 등록 ID를 사용한다. 권한 있는 주체가 기존 workspace의 cwd와 runtimeConfig를 새 준비 packet으로 명시적으로 갱신해야 한다. `transition`에 정확한 RC4 준비→새 후보 준비 pair를 지정한다. 이 경로는 **keys→keys만**, 새 cwd/bundle/artifacts/pin/final decision/두 revision/IPC/mailbox가 모두 다른 경우에 허용한다. worker·Chain·거래 중인 웹의 후보 교체는 지원하지 않는다. 이어서 최종 웹으로 바꾸면 `transition`을 `[이전준비→새준비, 새준비→최종웹]` 두 pair 배열로 보존하여 남아 있는 옛 service 행도 모두 대조한다.

이전 실행 객체가 종료된 경우 원 STOP 응답과 공개 mailbox를 `import-stopped-evidence --request /절대경로/import.json`으로 복사할 수 있다. import.json의 필드는 `packet`, `stop_response`, `mailbox_root`, `destination`이며 각각 이전 원문 객체/원문 응답/원본 mailbox 절대 경로/새 archive 절대 경로다. 부모는 private0700이어야 한다. 원본 RC4 mailbox는 이동·수정하지 않는다. 출력 receipt는 과거 증거일 뿐이며 새 승인이나 현재 자원 해제를 주장하지 않는다. 충돌·부분 기록은 보존하고 자동 재시도하지 않는다.


## RC6 — 보존 완료의 공개 경계

CTO-RC5-01 수정: `.<mailbox명>.archive-pending`을 mkdir하고 부모를 fsync한 뒤에만 원 mailbox를 그 안으로 이동한다. receipt/file·디렉터리·이동 후 부모 fsync 및 원문 대조가 모두 끝날 때까지 이 예약 디렉터리는 남는다. `Mailbox`/`WebMailbox`는 예약이 존재하면 새 nonce를 만들지 않는다. 오류·KeyboardInterrupt·프로세스 종료 뒤에도 자료를 삭제하거나 자동 재시도하지 않는다.

마지막 파일시스템 연산은 Darwin `renamex_np(RENAME_EXCL)`로 pending 디렉터리를 `completed-<nonce>`에 공개하는 것이다. 기존 목적지는 덮어쓰지 않는다. **이 원자적 rename이 완료 경계**이며, 그 뒤 오류를 낼 수 있는 fsync/검증을 실행하지 않는다. 완료 직후 응답 전에 프로세스가 종료되면 이미 공개된 영수증을 다음 프로세스가 읽는다. 최종 이름 자체의 전원 장애 내구성은 입증하지 않았고 별도 fsync 성공도 주장하지 않는다. 이름이 없거나 pending이면 재진입을 거절한다. 이는 종료 증거 공개 경계이며 `durable_ack=false`와 원 G00를 변경하지 않는다.

내부 종료 receipt는 `s3-local-stopped-receipt/2`와 정확한 `archive_root`를 요구한다. staging 경로·구 v1 receipt는 자동 인수/업그레이드하지 않는다. 읽기와 두 Reentry 검사/최종 confirm은 완료 경로와 pending 부재를 재확인한다. 기존 RC4/RC5 원 STOP/PID 자료는 원 mailbox가 온전할 때 명시적 import로 복사할 수 있으며, import 역시 모든 fsync 뒤 마지막 no-replace rename으로 게시한다. 실패한 pending/mailbox를 원본으로 삼거나 파일을 직접 이동·편집해 복구하는 경로는 없다. 불완전 자료는 보존하고 별도 조사한다.

`test_archive_publication.py`는 fee0/25 × 준비웹/최종웹/worker/Chain의 5 fsync와 2 rename 경계를 실제 파일시스템에서 주입한다. 정상·완료 직후 종료는 새 프로세스에서 인수하고, 완료 전 EIO/interrupt/exit는 receipt·nonce·Reentry·빈 receipt CLI의 session/START를 거절한다. STOP/PID/probe는 합성이며 실제 제품 서비스 재시작 시험은 NUS-74에 남는다.

## RC7 — 승인 지연과 challenge-only 실패의 재개

L-T-RC6-01: 긴 두 번째 승인 조회가 마지막 자원 관측의 2초를 소모하던 순서를 수정했다. 두 fresh 승인 조회를 모두 마지막 등록/자원 검사 앞으로 옮겼다. 준비 웹·최종 웹·worker·Chain에 동일하게 적용한다. 승인 callback은 매번 인증된 최신 native/CEO/CTO 문서와 후보 bytes를 확인하며 이전 성공 결과를 permit으로 재사용하지 않는다. 실제 2.1초 승인 지연은 이후 새 자원 관측으로 처리한다. final GET 자체가 2초를 넘거나 실패하면 START0으로 거절한다. 자원 probe의 기존 60초 총상한과 각 adapter 상한, writer 검사, managed child의 독립 승인 gate는 유지한다. final GET→START 비원자성 및 자원 지속 예약의 한계는 그대로다.

challenge만 있는 mailbox는 **실행되지 않았다는 증명도 정상 STOP receipt도 아니다**. 오류 뒤 같은 CLI를 반복해 nonce를 교체하지 않는다. `preserve`/`import-stopped-evidence`로 완료 자료를 만들거나 빈 receipts로 종료 이력을 숨기지 않는다. 기존 failed root/home/guard/genesis는 이동·수정·삭제하지 않는다.

이번 RC6 준비 웹 실패의 운영자 검토 및 명시적 재개 절차는 다음과 같다. 이 절은 L-R에서 서비스를 실행하는 명령이 아니며 새 승인 후 NUS-74에서 수행한다.

1. 실패 run의 stdout/stderr·오류 원인·request/등록 packet·runtime pin·승인 revision·challenge 원문/SHA·mailbox 파일 목록을 업무 artifact에 보존한다. 정상 종료 receipt들과 fresh 인증 workspace listing의 모든 service 행도 보존한다. challenge-only만 보고 START0으로 추정하지 말고 관리 operation/run 결과에서 START 요청 여부를 대조한다. 불명 요청·살아 있는 PID·누락된 STOP/PID·미해소 pending이면 재개하지 않고 조사한다.
2. NUS-73 새 후보 CTO→Security와 CEO/CTO 각 canonical 독립 승인, NUS-75 조합 gate를 확인한다. RC6 승인/pin은 재사용하지 않는다. 새 후보 checkout/bundle/artifacts와 새 private control root를 사용한다. 기존 실패 경로에 mkdir/rm/mv를 하지 않는다. 새 control root의 준비 packet을 `registration_render.py`의 기존 key-packet 생성 경로로 만들고, 권한 있는 주체가 **기존 준비 workspace ID**의 정확한 새 cwd/runtimeConfig를 등록한다. 완료된 RC6 설정을 같은 값으로 다시 쓰는 작업이 아니다.
3. 기존 service 행마다 최신 정상 STOP receipt를 유지하고 request의 `packet`은 새 준비 packet, `workspace_id`는 기존 ID, `receipts`는 위 완료 receipt 목록, `transition`은 정확한 이전 준비→새 준비 pair로 지정한다. 실제 RC4/RC6 행을 보존하려면 RC4→RC6, RC6→RC7 두 pair를 사용한다. RC7 정상 STOP 뒤 최종 웹으로 가면 RC7 준비→최종 pair를 덧붙인다. 이 알려진 이력을 위해 최대3개의 연속 pair를 허용하며 그 이상은 거절한다. 각 pair의 exact packet/설정·연결 순서와 모든 이전 행/receipt를 대조한다. failed challenge를 receipts에 넣지 않는다. 새 준비 packet은 cwd/bundle/artifacts/pin/decision/두 revision/IPC/mailbox가 모두 달라야 한다. 과거 receipt를 확인하는 것만으로 기동을 허가하지 않는다.
4. 운영자 검토 기록과 원문 해시를 NUS-74에 남긴 뒤 `python3 -B <새 candidate>/ops/s3-local/restart_session_cli.py run-reentry-reviewed --request <새 private request.json>`을 명시적으로 한 번 실행한다. 새 mailbox 경로에 새 nonce가 생성되고 두 fresh audit·두 등록/자원 검사·final GET·최종 인증 권한 재조회를 모두 다시 거친다. 거절 시 새 실패 자료도 보존한다. 성공 후 생성된 정상 종료 receipt만 다음 재진입에 사용한다.

현재 keys→keys 후보 전환만 지원한다. worker·Chain·최종 웹의 challenge-only 실패에 대해 home 교체나 후보 전환을 자동 적용하지 않는다. 이들의 불명 실행은 STOP/PID 원문과 자원 상태를 먼저 조사하고 소유 업무에 재개 조건을 기록한다. 이번 실제 실패는 준비 웹이므로 위 기존 전환 경로를 사용한다. 동일 후보/같은 mailbox의 수동 재설정 명령은 제공하지 않는다.

`test_reentry_freshness.py`는 네 역할 × fee0/25에서 실제 monotonic clock의 2.1초 지연과 합성 인증/제어/자원 IO를 구분해 기록한다. 거절 뒤 challenge 보존·기존 경로 새 nonce 거절, 이전 준비의 정상 receipt와 새 후보/새 nonce를 명시한 전환을 시험한다. 실제 실패 mailbox는 시험 입력으로 수정하지 않는다. 실제 managed 재시작·DEV/AR 제품 판정은 새 pin 이후 NUS-74에서 수행한다.


## RC8 — 마지막 자원 검사 이후 승인 철회 차단

SEC-RC7-01을 수정한다. RC7의 두 전체 audit를 앞당긴 것만으로는 두 번째 probe/final GET 중 철회를 검출하지 못했다. 순서는 등록/자원1 → 전체 audit1 → 전체 audit2 → 등록/자원2 → final GET → **새 인증 권한 재조회** → confirm → START다. 준비 웹·최종 웹·worker·Chain 모두 같은 경로를 사용한다.

마지막 `approval_gate.recheck`는 current-run Reader(또는 같은 run의 private transport)로 native 상태와 CEO/CTO canonical 문서를 새로 읽는다. native 완료/판정·작성자·최신 revision·approved 원문과 subject SHA를 다시 검증한다. subject 자체도 최신 문서에서 복원하며 앞선 전체 audit의 SHA는 비교할 후보 식별자일 뿐이다. 이전 approved flag나 문서 원문을 실행 허가로 재사용하지 않는다. 큰 binary/manifest를 다시 hash하지 않아 이 경로는 전체 byte audit와 분리된다. 두 전체 byte audit 및 child의 독립 검증은 보존한다.

2.1초가 걸린 **전체 audit1/2**는 그 뒤 새 자원 검사로 처리한다. 반면 **final GET과 최종 권한 조회를 합친 시간**이 마지막 자원 검사 후 2초를 넘으면 안전하게 거절한다. 시간 기준/60초 probe 상한을 늘리거나 clock을 조작하지 않는다. 느린 최종 인증 API에 대한 성공 보장은 없으며, 실제 NUS-74에서 관측해야 한다. 두 번째 자원 검사/final GET에서 CEO·CTO·native 철회/교체가 발생하면 START0이다. API가 원자 compare-and-start를 제공하지 않아 마지막 인증 응답 이후의 경쟁까지 제거했다고 주장하지 않는다(SEC-RC6-R2 유지).

각 Reentry session 시도는 시작 전에 소모된다. 마지막 권한 callback 누락/실패/interrupt, freshness 초과를 포함해 같은 객체를 다시 실행할 수 없다. challenge-only는 기존 보존·운영자 조사·새 후보/새 경로/새 nonce 절차를 따른다. 이번 RC7은 서비스가 시작되지 않았으므로 실제 stopped 행은 RC4/RC6 그대로이며 전환은 RC4→RC6→RC8 준비→최종 웹이다. 실패한 RC7을 stopped 이력으로 추가하지 않는다.

`test_reentry_freshness.py`는 실제 clock/filesystem 및 합성 IO에서 네 역할×fee0/25의 post-audit2 probe/final GET 철회·교체, 최종 인증 지연/IO/interrupt와 원 challenge/재호출 거절을 검증한다. 마지막 권한 판정 함수는 실제 production decoder/metadata/native/document 검사이며 API reader만 합성이다. 실제 제품/관리 START0·DEV/AR NOT_RUN·G00 FAIL_UNPROVEN·durable_ack=false·€0를 유지한다.
