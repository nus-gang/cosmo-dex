# RC9 실패 이후 새 실행 후보의 경계

이 문서는 NUS-73의 기동 전 수정 후보다. 새 CTO→Security 판정, NUS-75의 같은 subject 독립 CTO/CEO 승인과 실행 pin이 마련되기 전에는 아래 L-T 함수를 호출하지 않는다. RC9 승인을 변경된 파일에 재사용하지 않는다. `DEV/AR NOT_RUN`, `durable_ack=false`, `G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`를 유지한다. 이번 L-R 검증은 컴파일·합성 전송·자식 프로세스·거절/정리 시험이며 개발 Chain/worker/web/Chrome 서비스 시작은 없다.

판정 출처: [CTO RC9 triage](/NUS/issues/NUS-73#document-rc9-recovery-triage-cto), revision `83c6e7c1-e0fa-4d41-a9e1-0dc0159afaa5`. 당시 실제 bootstrap 실패 단계·RPC 송수신과 탭 종료 신호는 미확정이다. 새 진단으로 과거 원인을 소급 확정하지 않는다.

## 1. BOOTSTRAP-01 — 한 시도의 단계와 원문 보존

L-T의 유일한 실행 경로는 기존 `bootstrap_initialize_cli.py` → 인증된 capture/checker → fetch 한 번 → 별도 승인 재조회 → C create다. RPC나 create를 자동 재시도하지 않는다. RPC 총 상한 2초, 부모 fetch 기존 상한, raw/cap, C의 경제·서명·guard 검증을 유지한다.

별도 root0700 증거 디렉터리에 다음을 no-replace로 남긴다.

| 파일 | 의미 |
|---|---|
| `bootstrap-diagnostic.jsonl` | 승인/capture, 예약·fsync, checker, child PID/exit, 실제 connect/send/read, framing, create 단계. 단조 시계·바이트 수·hash와 고정 코드만 기록한다. exception/stderr/API 토큰은 저장하지 않는다. |
| `snapshot-fetch.wire` | 자식이 실제 read한 HTTP wire. 16MiB+256KiB 상한. 절단/오류 원문도 보존하며 Snapshot 입력으로 사용하지 않는다. |
| `snapshot-fetch.raw` | 자식 stdout의 HTTP entity. 빈 파일은 송신0이나 RPC0을 뜻하지 않는다. 완전한 성공 entity만 기존 C 경로에 넘긴다. |

자식은 두 전용 pipe로 진단과 wire를 보낸다. parent는 caps·고정 schema·단조 child elapsed·완료 byte count를 확인하며 deadline/stop에서 kill/reap한다. checker 거절과 전송 오류가 같은 공개 오류여도 private 단계는 분리된다. 저장 오류가 나면 남은 부분 원문과 journal을 보존하고 create를 닫는다. 진단 저장 자체의 실패는 실제 전송 여부를 불명으로 남길 수 있다. journal 끝이 없으면 성공으로 보지 않는다.

## 2. STOP-02 — 네 STOP 뒤에 자원 관측

L-T에서 `authenticated_chain_group.session(...)`을 사용한다. 네 exact packet, workspace ID, 새 mailbox, 승인·재진입 근거를 모두 준비하고 B의 전체 topology checker를 통과한 뒤 시작한다. 개별 `authenticated_chain_session`의 `finally`는 대상 STOP을 한 번 요청하고 fresh control-plane 상태를 저장한다. 그룹의 네 STOP 처리가 끝나야 PID→전체 포트→writer 관측을 시작한다. 시작 응답 유실도 해당 STOP을 생략하지 않는다.

전체 포트 집합에 한 번의 60초 예산을 적용한다. 첫 관측과 250ms 간격 최대241회, **EADDRINUSE만** 재관측한다. 다른 errno·시계 역행·interrupt·close 오류는 즉시 관측 실패다. listen/connect/reuse 옵션은 사용하지 않고 매번 모든 probe socket을 닫는다. 네 노드별로 60초를 곱하지 않는다.

STOP 확인과 PID 부재가 성립한 노드에서는 포트 관측 실패와 별개로 writer lock을 확인한다. STOP/PID가 불명인 노드는 writer를 열지 않는다. 본문 예외 객체는 그대로 다시 발생시키고, `chain-group-result.json`과 `chain-group-publication.json`에 STOP·관측·archive 오류를 별도로 남긴다. 본문 실패가 있으면 정상 receipt를 생성하지 않는다. 정상 경로도 완전한 종료 증거가 없는 노드는 archive하지 않는다. 이 관측은 전체 프로세스 트리/지속 exclusion/전원 장애 내구성을 보장하지 않는다.

### 기존 RC9 실패 4개만을 위한 archive

`failed_chain.preserve(destination)`은 검토된 사건 hash `b34cc16f5723c13275b5eea259fde11939c8b1dcc7f551e3a145fb512626fc08`에만 적용된다. 원 mailbox4의 nonce/PID bytes, STOP 응답4, 빈 snapshot, 당시 실패와 후행 자원 관측을 그대로 확인한다. 원 home/mailbox를 이동·수리·삭제하지 않는다. 새 목적지 이름은 `failed-chain-rc9-b34cc16f5723c132`이며 pending 충돌·원 증거 변경은 거절한다.

```python
from pathlib import Path
from failed_chain import preserve
failure = preserve(Path(archive_root) / 'failed-chain-rc9-b34cc16f5723c132')
# 각 v0..3의 기존 packet → 새 승인 packet을 정확히 연결한다.
from workspace_reentry import Reentry
entry = Reentry(new_packet, [], transition={'previous': old_packet, 'next': new_packet},
                failed_chains=[failure])
```

`archive_root`는 L-T의 새 private 증거 루트다. 이 파일은 `normal_receipt=false`다. 재진입은 새 source/bundle/artifacts/pin/결정/문서 revision/home/입력/mailbox와 같은 기존 workspace ID를 요구한다. 현재 API의 원 service row를 하나도 지우지 않고 대조하고, 현재 PID/port/writer와 최종 승인 freshness를 다시 확인한다. 원 RC8 실패 예외를 일반화하지 않는다. RC9 후행 PASS를 정상 receipt로 합성하지 않는다.

## 3. BROWSER-03 — 등록을 끝낸 뒤 한 관리 프로세스에서 탭 보유

기존 run 내 REPL keeper와 키 준비용 web을 중간에 내렸다 올리는 절차 대신 `browser_web_cli.py serve-browser-reviewed`를 기존 web workspace 명령으로 사용한다. 관리 Python 부모→Node 감독→기존 Chrome→새 탭을 최대300초 한 foreground 수명으로 묶는다. 기존 Chrome의 실행 파일뿐 아니라 설치 bundle697파일·7내부 링크를 descriptor artifact에 pin한다. 설치 변경이나 drift 시 시작을 거절한다. 새 설치·개인 브라우저 프로필·외부 CDP 포트는 없다.

`planned_registration_cli.py plan-before-keys`는 새 key/node ID 없이 경로·포트·pin·승인 참조·명령12개를 결정한다. 입력 형식은 기존 `s3-local-initializer-registration/1`이다. Chain argv의 `--topology-root`는 후에 승인 initializer가 만든 공개 node ID를 읽고 B가 실제 home과 다시 비교한다. 이 값은 키나 B 검증을 대체하지 않는다.

```sh
python3 -B ops/s3-local/planned_registration_cli.py plan-before-keys < registration-input.json > planned-registration.json
```

등록은 기존12개 workspace를 재사용한다. `planned_registration.diff(planned, authenticated_listing)`이 실제 달라진 필드의 PATCH만 출력하며 원 service rows는 보존한다. POST12개를 반복하거나 등록 권한을 우회하지 않는다. 최종 승인 참조가 발급된 뒤 이 차이를 권한 있는 등록 경로에 전달한다. **등록 대기가 끝나기 전 사용자 키를 생성하지 않는다.**

L-T의 `authenticated_browser_session.session(planned, packet, workspace_id, mailbox, history_factory)`는 fresh authenticated listing으로12개 등록과 이전 정상/실패 history를 확인한 다음에만 broker와 web START를 연다. `history_factory(packet)`은 매 호출 새 `Reentry`를 반환한다. 준비 확인에서 소진한 객체를 실제 start에 재사용하지 않는다. 기존 web history에는 원 정상 receipt와 RC8 실패 archive 및 실제 RC9 정상 종료 receipt를 모두 유지한다.

```python
from browser_control import wait_ready, command, activate
wait_ready(control)                         # 최대60초, 키 없음
command(control, 'prepare')                  # 새 탭의 공개 등록값만 파일로 전달
# control/registration.json의 public_key_base64 두 개를 users.json에 투영한다.
# 승인 initialize_cli로 fee별 새 genesis/4 home을 만든다. 공개 topology를 저장한다.
from planned_registration import publish_topology
publish_topology(profile_root)
# authenticated_chain_group에서 네 노드를 시작한다.
# bootstrap_initialize_cli 한 번으로 C engine 생성, 인증 worker session 시작.
activate(control, runtime_pin)               # 같은 tab/doc, 같은 등록키와 C Context 대조
command(control, 'inspect')
# 승인된 사용자 UI 동작·검증 후, worker/chain을 종료·관측한다.
command(control, 'finish')                   # Chrome 정상 종료와 공개 PID 증거
```

이 코드는 L-T 순서를 설명한다. 실제 승인 IDs·절대경로·12개 기존 workspace ID는 새 pin 인계의 `registration-input.json`에 고정한다. 등록 API 완료가 keys 이후 필요해지면 진행하지 않는다. 300초 안에 준비/시연이 끝나지 않으면 실패를 보존한다. 자동 연장·탭 재생성·키 export·기존 탭 복구·home reseed는 없다. 같은 실패 home으로 자동 재실행하지 않는다.

감독은 500ms마다 새 nonce에 응답하는 동일 document/origin을 검사한다. CDP 요청은2초 상한이고 disconnect/navigation/부모 소실/수명 초과는 닫힌 상태다. `WAIT_KEYS → KEYS_HELD → ACTIVE → CLOSED` 외 상태 전이를 거절한다. ACTIVE 전 거래 동작0. 동일 공개 등록 hash와 승인 capture의 genesis 공개키 배열 및 정확한 Context를 비교한 후 같은 페이지에서 활성화한다. 준비/최종 HTML·JS 한 바이트라도 바뀌면 거절한다.

`parent.json`, 번호별 고정 event, command nonce/result, `parent-closed.json`은 새 root0700의 파일0600으로 보존한다. stdout/stderr·private key는 기록하지 않는다. 부모/Node/Chrome PID와 탭 ID/document nonce를 분리한다. `browser_inventory`는 명시된 세 PID와 종료 evidence만 확인하며 전체 Chrome 자손 트리 종료는 여전히 별도 L-T 관측 대상이다. 실제 Chrome/4노드/거래의 생존 시간과 DEV 인수는 L-R 합성 시험으로 입증하지 않는다.

## 4. 검토·재현

새 후보는 source commit/tree, 기존 승인 component bytes/실제 lock, build argv·도구·native binary/web artifact SHA, 다섯 descriptor, manifest를 한 묶음으로 제출한다. manifest 자기 자신·genesis·키·실행 결과는 집계 밖이다. 소스의 사건 fixture는 과거 공개 증거이며 실행 home/키가 아니다.

순수 Python 회귀는 `ops/s3-local`에서 `python3 -B -m unittest test_recovery_rc10 test_browser_runtime test_bootstrap_diagnostic`로 시작한다. `browser_supervisor.test.mjs`는 고정 Node24.21.0의 `--test`로 실행한다. Rust bootstrap-fetch 시험은 release의 정확한 extern/feature와 실제 checker/create/fetch binary를 사용하며 `NUS_TEST_TMPDIR`를 run scratch로 지정한다. 실제 네트워크/서비스에 연결되는 테스트로 바꾸지 않는다.

## RC11: RC10 브라우저 실패 반환

Wallet `04fa4108aa4b02e2677f287e8bff06cf5ece9364`의 CTO→Security 완료를 인수했다. 기존 public/trusted 계약·서명·경제 로직을 변경하지 않는다. 새 실행 후보와 독립 pin을 받기 전 NUS-74 START는 금지다. 원 RC10의 0.730193초 실패 원인은 `UNPROVEN`으로 보존하며 이번 합성 재현으로 실제 원인을 확정하지 않는다.

`browser_supervisor.mjs`는 target create/attach, Page.enable, viewport, readiness, frame/loader, 문서 nonce, context HTTP 및 UI 단계의 최초 실패와 마지막 완료를 남긴다. 원 CDP 메시지·본문·임의 JS·키/token은 저장하지 않는다. `closed`의 exit/signal/close 오류는 최초 실패와 별개다. 준비 전/ACTIVE 이후 단계와 300초 단조 수명·부모 연결·nonce/origin·등록/Context 검사는 유지된다. 실제 Chrome 300초·모든 자손 종료는 L-T에서 검증하며 현재 미입증이다.

고정 UI 명령은 실제 Wallet `aria-label` 대상의 유일성·disabled·option/value를 확인한다. `input`의 이름은 `예치 금액`, `출금 금액`, `주문 수량 lots`, `지정가 ticks`다. 금액은 6자리 소수까지의 표시 단위이며 Wallet이 atoms로 바꾼다. 주문은 계약의 정수 lots/ticks다. `select`는 `계정`(0/1), `예치 자산`/`출금 자산`(DEVBASE/DEVQUOTE), `주문 방향`(BUY/SELL), `주문 유형`(GTC/IOC), `최대 수수료 bps`(0/25), `취소 주문`(화면에 존재하는 order_id:order_hash)만 허용한다.

`click`은 인증·상태 조회, 직접 예치, 주문·잔량 취소, 원 결과 조회, 출금 준비/해제, 직접 출금 및 결과 조회의 실제 버튼명만 허용한다. 자동 재서명·재전송·임의 eval·키/client export API는 없다. 동작 응답의 `dispatched`는 클릭 전달만 뜻한다. 완료 판정은 별도 `snapshot`과 승인 체인 증거를 대조한다. `wait_screen(control, section, text, seconds=10)`은 최대10초의 화면 조회만 수행하고 계정이 바뀌면 거절한다. `snapshot`은 공개 status/identity/ledger/orders/fills/batches/commands/history/receipt와 버튼/선택 목록을 최대48,000 UTF-8 bytes까지 반환한다. `screenshot`은 ACTIVE에서만 고정1280×900 화면을 최대1MiB PNG(세션당16개·총16MiB 상한)로 전용 `screenshots/`에 no-replace 저장하고 SHA/길이만 응답한다. 페이지나 스크린샷을 trusted receipt로 취급하지 않는다.

L-T의 명시적 두 사용자 순서는 계정 선택·인증 → A DEVBASE10/B DEVQUOTE100 예치 → 각 결과 조회·최신 C 확인 → A SELL GTC2@10 → B BUY GTC1@12 → A의 화면 주문 ID/hash로 잔량 취소 → 정산 COMMITTED 및 최신 C/R/D/P 대조 → A QUOTE10/B BASE1(25bps는9.975/0.9975) 직접 출금이다. `주문·취소 원 결과 조회`는 UNKNOWN 원 요청만 조회한다. `출금 결과 조회`는 예치/출금 DIRECT 이력의 확정 조회도 수행한다. 버튼이 보류되거나 UNKNOWN이면 다음 자산 동작으로 넘어가지 않는다. 체인·자산 보존/서명·영수증 최종 판정은 NUS-74에 남는다.

### RC10 실패 archive와 새 후보 재진입

`failed_browser.preserve(new_archive)`는 `failed-browser-rc10-9ab13f50a231472f1` 이름의 새 디렉터리에만 원 사건을 복사한다. 원 `/private/tmp/paperclip-nus74-rc10-6108a4cb/fee0/web/mail`과 `browser`는 읽기만 하며 nonce 교체·이동·삭제·수리0이다. 원문 SHA, challenge/PID, spawned→failed→closed 및 부모 종료 기록을 대조한다. 원 browser/commands에 남은 미소비 finish 요청1개(결과0)도 원 nonce/bytes 그대로 복사·대조한다. 응답이 없으므로 정상 종료 receipt로 간주하지 않는다. `normal_receipt=false / original_cause=UNPROVEN / cleanup_complete_verified=false`다.

NUS-74의 기존 history 입력·정상4영수증·RC8 failure와 기존 transition4개를 그대로 읽는다. 새 후보 packet을 만든 뒤 **기존 RC10 browser packet→새 browser packet** edge를 한 개 덧붙이고 `Reentry(..., failed_browsers=[archive/'failure.json'])`에 전달한다. 기존 RC9 chain failure4개와 다른 workspace 이력은 기존대로 유지한다. 새 browser 후보는 runtime pin/native decision/CEO·CTO revision, source/bundle/artifacts, home/input/key/mailbox/control을 모두 새로 고정해야 한다. 새 pin 없이 이 도구를 기동 허가로 쓰지 않는다.

현재 웹 workspace의 과거4행+실패 service `99e45ba4-df12-468f-ae99-4c9ce6c79b05`를 전부 대조하며 누락·중복·새 행·변조·running을 거절한다. 모든12 workspace의 최신 등록/행 확인과 해당 재진입의 두 번 자원 관측·최종 GET→새 승인 audit 순서를 유지한다. PID22228/22324/22325와5173의 새 부재/가용 관측이 필요하다. 실제 모든 Chrome 자손 종료는 이 목록으로 보장하지 않는다. 과거 실패를 정상 종료로 승격하지 않는다.

기동 없는 최소 회귀는 `python3 -B -m unittest test_browser_runtime test_failed_browser`와 Node24의 `--experimental-strip-types --test browser_supervisor.test.mjs browser_dom.test.mjs browser/page.test.ts browser/entry.test.ts`다. 실제 main/CDP pipe·child reap은 합성 child, DOM은 기존 설치 jsdom+승인 Wallet mount다. 새 네트워크 서비스·Chrome는 시작하지 않는다. 기존 workspace 등록12개는 반복 요청하지 않으며 새 후보의 명령 bytes 차이는 최종 pin 이후 기존 권한 경로로만 인계한다.

CTO→Security의 새 네이티브 cycle이 동일 후보를 승인한 다음 NUS-75의 canonical 승인·composed gate를 거쳐 NUS-74를 재시험한다. 과거 승인 문서는 역사다. 남은 보안 요구와 표준 부모 blocker는 유지한다.
