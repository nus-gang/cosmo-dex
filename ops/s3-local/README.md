# L-R 사전 빌드 입력 도구

이 디렉터리는 [NUS-73](/NUS/issues/NUS-73)의 진행 중 산출물이다. **실행 launcher·최종 runtime manifest·pin은 아직 완성되지 않았다.** 서비스 시작 명령이 아니며 DEV01~14는 NOT_RUN이다.

**주의:** `manifest.py`는 공개 계정 영수증 A와 승인 B/C/D/E의 고정 참조를 사용한다. D가 승인한 `exchange/` adapter bytes와 C ancestry를 별도로 기록한다. 최신 판정/revision 확인과 독립 runtime 심사 출처 결합은 별도로 필요하다. 현재 component 또는 계약 소스 불일치는 봉인을 거절하며 자동 승인 예외를 두지 않는다.

변경(2026-10-08): 공개 `s3-dev-local-account/1`의 manifest/schema pin과 61개 계약 파일을 trusted `s3-dev-local/1` 입력에 추가했다. old/new fallback은 없고 서비스 활성화·runtime 승인은 여전히 별도다.

`manifest.py audit`는 고정 A/B/C/D/E의 ancestor와 후보 commit의 component·계약 원문/mode/누락을 대조한다. 원 A Git snapshot에서 기존 213개와 공개 계약 61개를 읽어 승인 SHA를 대조한다. 현재 구현 lock으로 계약 snapshot을 다시 봉인하지 않는다. 실제 구현 lock은 별도로 기록한다. clean checkout이 필요하다.

```sh
python3 ops/s3-local/manifest.py audit --source . --out /existing-parent/new-audit
python3 -B -m unittest discover -s ops/s3-local -p 'test_*.py' -v
```

`seal`은 `--build-spec spec.json --artifacts /build-root`를 추가로 받는다. spec의 키는 정확히 chain/exchange/settlement/wallet/sre다. 각 값에는 `build_argv`(문자열 배열), `toolchain`(문자열), `artifacts`(build-root 상대 파일 배열), `approval_sources`(출처 배열), `settings`(문자열 map)가 필요하다. 원본 lock/실제 lock·정확한 build argv·toolchain·파일 SHA가 5 descriptor의 문자열 설정에 결합된다. 실행 파일, 웹 파일과 launcher를 빠짐없이 열거하는 책임은 빌드 인계와 CTO→Security 심사에 있다. sealer는 경로가 있다고 실행 가능한 서비스인지를 보증하지 않는다.

파일 집합은 상속 274개와 descriptor 5개만이다. 실제 binary는 descriptor 안의 SHA로 결합하며 집계에 파일 자체를 추가하지 않는다. 집계는 경로 정렬 후 `sha256 + two spaces + path + LF`의 SHA256이다. runtime manifest 자신·genesis·key·실행 결과를 집계에 넣지 않는다. 출력 디렉터리 재사용과 symlink/hardlink artifact를 거절한다. `seal`은 모든 descriptor를 만든 뒤 실제 artifact SHA를 다시 읽고, 마지막 source audit의 head/tree/lock/상속 원문이 시작 시점과 같은지 검사한다. 변경·삭제·링크 교체·dirty source면 후보를 반환하지 않는다. 이는 관측 시점의 일관성 검사이며 경로 잠금이나 이후 바이트 불변 보장은 아니다. 출력 이후 실행 시점의 capture/독립 승인 대조도 필요하다.

codec가 요구하는 `scope=REVIEWED_RUNTIME`은 형식 필드이며 이 도구의 승인 선언이 아니다. `audit.json`의 `runtime_approved=false`, `candidate_runtime_manifest_sha256`은 검토 전 후보 기록이다. 이 값을 스스로 `approved_runtime_sha256`에 복사해 서비스를 시작하지 않는다. CEO/CTO의 독립 승인 출처와 동일 후보 CTO→Security 완료가 있어야 pin으로 인수한다. 한 바이트라도 바뀌면 새 manifest/심사가 필요하다.

검증 시험의 `NOT_A_RUNTIME_BINARY`와 `TEST_ONLY_NOT_APPROVED`는 합성 바이트 변조 시험이며 runtime 산출물로 인계하지 않는다. 새 서비스·home·genesis·키 생성, 표준 G00/ACK 변경, 새 설치나 main 병합을 수행하지 않는다.

## 실제 파일 바이트 preflight (기동 전 부분 구현)

```sh
python3 -B ops/s3-local/preflight.py \
  --bundle /absolute/candidate-bundle --artifacts /absolute/build-root \
  --runtime-pin <independently-approved-manifest-sha256> \
  --local-demo-profile s3-dev-local/1 --acknowledge-unproven-space
```

이 명령은 파일 읽기만 한다. 두 opt-in·manifest 원문 SHA·고정 A manifest 세 개·상속 274파일과 정확한 5 descriptor·집계·descriptor 내부 실제 artifact SHA를 확인한다. 공개 receipt manifest/schema/version pin도 독립 필드로 대조한다. canonical root·상대 경로·각 경로의 no-follow fd 탐색·single regular file을 요구하며 symlink/hardlink/FIFO·파일 변경·크기 초과를 거절한다. 산출물당 512MiB 상한, 해시 메모리는 1MiB다. 네트워크·port bind·home/key 생성·서비스 실행·승인 발급은 없다.

성공 출력은 `byte_match=true`, **`approval_verified=false`**다. 입력 pin의 독립 승인 여부와 CTO→Security 판정은 control plane에서 별도로 확인해야 한다. B/C의 guard/genesis/profile 검증, 실행 직전 재대조, 실제 runtime 등록을 대체하지 않는다. 검사 후 파일 변경 가능성이 있으므로 이 결과만으로 나중 실행을 허가하지 않는다. exact executable 목록의 완전성은 최종 후보 심사 대상이다.

`python3 -B -m unittest discover -s ops/s3-local -p test_preflight.py -v`에서 **11 PASS / 0 FAIL**. 승인 상속 파일을 Git A에서 읽고 합성 descriptor·`NOT_A_RUNTIME_BINARY`를 사용한 순수 파일 시험이다. 이 fixture pin은 승인 runtime pin이 아니다. 원문/산출물 변조, reseal한 상속 파일 교체, component 경로, 두 opt-in, 중복 JSON, alias/링크/FIFO/크기 상한 거절을 검증한다. 서비스0·DEV NOT_RUN이다.

worker/proof/signer·웹 ChainPort·프로세스 launcher·fee0/25 초기화·정리/fault driver·최종 manifest 및 독립 승인은 아직 남아 있다.

## Rust input-set 바이트 결합

`preflight.verify_input_set(bundle, artifacts, pin, profile, acknowledge, inputs, input_name)`은 기존 실제 artifact 검증 후 Rust `Validated::decode_bundle` 전송 입력의 manifest 원문과 정확한 279 파일을 같은 pin에 결합한다. 중복 JSON·추가/누락 파일·다른 manifest·비정규 base64·빈/과대 guard/genesis를 거절한다. 입력 파일은 canonical root의 no-follow/single-link/bounded 읽기이며 반환값은 읽은 원문 bytes와 SHA256이다. 후속 launcher는 이 캡처를 사용해야 하며 path 재읽기를 승인된 입력으로 간주하면 안 된다.

`input_set_byte_match=true`는 B/C 의미 검증이나 조직 승인 결과가 아니다. guard/genesis 내용과 effective profile은 기존 B/C에서 검증해야 한다. `semantic_validation=false`, `approval_verified=false`를 유지한다. 캡처 출력과 Rust 수신 API는 아래와 같다. 독립 승인 gate·executable 호출·서비스 실행 연결은 아직 미완성이다.


### 캡처 전송 (기동 없음)

`preflight.py`에 기존 필수 인자와 `--capture-input /absolute/input.json`을 주면 검증 완료 뒤 input-set 원문만 stdout에 출력하고 report는 stderr에 출력한다. 실패 시 stdout은 비어 있고 exit 2다. 부모 launcher는 제한된 출력 수집·종료 성공을 먼저 확인하고 report의 `input_set_sha256`와 수집한 원문을 Rust `Inputs::prepare_captured(reader, capture_sha256)`에 전달해야 한다. 이 SHA는 전송 무결성일 뿐 독립 승인이 아니다.

Rust는 48MiB 상한·EOF·SHA를 확인한 후 C의 `Validated::decode_bundle` 및 기존 home 열기 경로로 연결한다. `--input-set` 경로는 이 경로에서 다시 읽지 않는다. effective profile은 별도 지정 파일을 읽고 C가 의미를 검증한다. 기존 `prepare()`는 경로 기반 component 시험 API로 유지하며 launcher의 검증된 캡처 대신 호출하지 않는다. 살아 있는 비동기 stream을 직접 전달하지 말고 부모가 제한시간 내 완전히 수집한 메모리/닫힌 pipe를 사용한다.

합성 fixture를 사용한 Rust 준비 시험과 Python CLI 시험이며 실제 Python→worker executable 실행은 아직 연결하지 않았다. 서비스/포트/RPC 실행 없음.

### 캡처와 descriptor 기반 offline validator 연결

`offline_check.check(bundle, artifacts, pin, profile, acknowledge, inputs, input_name, arguments, scratch, timeout=60)`은 검증한 input-set bytes의 SRE descriptor에서 `bin/s3-local-preflight` SHA를 읽는다. 별도 경로 재조회로 descriptor를 바꾸지 않는다. 실행 파일도 no-follow/single-link/bounded 읽기 후 SHA를 대조하고, `scratch` 아래 임시 root0700에 mode0500 사본을 만들어 기존 `process_check.validate_captured`로 실행한다. 원본 경로가 교체되어도 이 사본을 실행하며 완료/오류/중단 후 사본만 제거한다. home/key/WAL/입력과 원본 binary는 제거하지 않는다. `scratch`는 호출자가 제공한 Paperclip scratch의 canonical 경로다.

바이너리는 최대512MiB를 메모리에 캡처한다. 동일 UID 또는 interpreter/동적 loader에 대한 sandbox가 아니며, 최종 Mac native binary 재현·서명/loader 조건은 후보 심사에 남는다. API는 조직 승인을 확인하지 않는다. 최종 launcher가 독립 승인 출처를 검사해야 하며 서비스 실행 명령은 제공하지 않는다.

`python3 -m unittest discover -s ops/s3-local -p 'test_*check.py' -v`: 신규5+기존 감독5 = **10 PASS / 0 FAIL**. 합성 script validator와 합성 guard/genesis를 사용하며 실제 C 의미 검증을 입증하지 않는다. 실제 Rust validator와 이 API의 end-to-end 시험·최종 CLI/승인 gate·서비스 executable·웹 ChainPort·초기화/fault/정리·최종 manifest는 후속 작업이다. 서비스0·runtime pin 미발급·DEV NOT_RUN.

### Offline CLI (서비스 기동 없음)

```sh
python3 -B ops/s3-local/offline_cli.py \
  --bundle /absolute/candidate-bundle --artifacts /absolute/build-root \
  --input-set /absolute/inputs/input.json \
  --runtime-pin <candidate-manifest-sha256> \
  --local-demo-profile s3-dev-local/1 --acknowledge-unproven-space \
  --effective-profile /absolute/inputs/effective-profile.json \
  --home /absolute/existing-home --key-directory /absolute/private-keys \
  --scratch "$PAPERCLIP_RUN_SCRATCH_DIR" \
  --bind 127.0.0.1:18080 --rpc 127.0.0.1:26657 \
  --lifetime-seconds 60 --max-requests 10 --max-ticks 10
```

`offline_cli.py`는 위 capture→descriptor SHA→임시 실행 사본→Rust/C 의미 검증을 호출한다. `--local-demo-profile`은 명시적 opt-in 이름이고 `--effective-profile`은 Rust/C가 읽을 실제 설정 파일이다. 기존 home·private signer가 필요하며 새 home을 만들지 않는다. `bind/rpc`는 준비 단계 설정 검증만 하며 포트를 열거나 RPC를 호출하지 않는다. 이 명령은 pin의 독립 승인을 확인하지 않고 항상 `approval_verified=false`, `services_started=false`, `DEV=NOT_RUN`을 보고한다. 임의 명령/추가 인자 전달·축약·중복·`--serve`를 거절한다. 실패는 exit2·stdout0·고정 진단이며 경로/키/하위 오류를 출력하지 않는다.

CLI 경계 신규4 및 synthetic child 전체 연결1을 시험했다. 실제 Rust/C capture 연결은 이전 API 시험의 근거이며 이번 synthetic CLI 시험을 실제 서비스/독립 승인/DEV PASS로 해석하지 않는다. 서비스 executable·독립 승인 gate·웹 ChainPort·초기화/launcher/fault/정리·최종 manifest는 남아 있다.


### 독립 심사용 원문 결합 (승인 인증 전 단계)

`review_subject.subject(bundle, artifacts, pin, profile, acknowledge)`는 기존 byte preflight 뒤 manifest 원문과 다섯 descriptor 원문을 다시 SHA 대조하여 캡처한다. 반환되는 canonical JSON은 원문 base64·manifest SHA·component head/tree·build argv·toolchain·실제 구현 lock·component 승인 출처·binary/web SHA 목록을 포함한다. build 정보가 비어 있으면 거절한다. genesis/key/home/실행 결과는 포함하지 않는다. 기존 218파일 집계와 descriptor schema는 바꾸지 않는다.

CEO/CTO 독립 승인 출처에 **같은 반환 원문과 SHA**를 결합할 수 있다. `compare(current, independent_raw, independent_sha256)`는 원문을 바이트 단위로 대조하며, self-asserted hash를 조직 승인으로 승격하지 않는다. 279파일 집합에도 항상 `approval_verified=false`, `services_started=false`, `DEV=NOT_RUN`이다. 승인 출처의 작성자/역할·최신 revision·철회·CTO→Security 완료를 확인하는 실제 gate는 별도다. 파일의 현재 바이트 확인은 그 시점의 관측이며 향후 서비스 실행 허가가 아니다. 최종 launcher는 실행 직전 재검증/캡처를 유지해야 한다.

시험: `python3 -B -m unittest discover -s ops/s3-local -p test_review_subject.py -v` — 합성 descriptor 5 PASS/0 FAIL. 원문 포함·artifact 목록 결합·self-hash/공백 변경·검증 후 manifest/descriptor 교체·binary 변조·build metadata 누락 거절을 확인했다. 실제 승인/서비스/DEV 시험이 아니다.

### 네이티브 검토 상태 검사 (부분 구현)

`native_review.inspect(read_issue, expected_decision_id)`는 고정 회사/NUS-73의 최신 서버 응답을 읽는 trusted reader를 받는다. CTO→Security 정확한 순서·각 1승인·commentRequired·심사자 ID·서로 다른 stage/participant ID·done/completed/approved·같은 순서의 완료 stage와 독립 인계의 최종 판정 ID를 요구한다. 두 번 조회 사이 재개/반려/판정/정책 변경 또는 IO 오류는 거절한다. 재조회는 관측 사이 경합을 탐지할 뿐 향후 철회 방지나 원자적 실행 허가가 아니다.

이 모듈은 reader를 인증하지 않는다. 로컬 JSON/캐시를 reader에 넣어도 조직 승인으로 사용할 수 없다. 실제 인증된 control-plane reader, CEO/CTO 원문 attestation 문서의 작성자·최신 revision·철회 확인, 판정과 exact 후보의 결합은 아직 미연결이다. 결과는 계속 `approval_verified=false`이며 CLI/서비스 기동 허가로 사용하지 않는다. 기존 완료된 C 응답의 실제 필드 구조를 참조했으나 그 승인을 NUS-73으로 전용하지 않는다.

2026-10-07: 신규 순수시험5+원문 결합 회귀5 = 10 PASS/0 FAIL. 합성 서버 응답의 정상/거절/경합/IO 검증이다. 첫 실행의 작업 디렉터리 오류와 로그 출력 상대 경로 오류를 보정했으며 시험 실패와 구분해 기록했다. 서비스0·pin 미발급·DEV NOT_RUN.

### 관리 runtime 인증 transport (L-R 준비)

`runtime_client.RuntimeClient.from_environment(workspace_id, command_id)`는 현재
run 환경의 인증·run ID를 사용한다. `read_workspaces`와 `request`를
`managed_session._session`의 trusted adapter로 연결할 수 있다. 허용 범위는
고정 프로젝트 workspace 목록 GET과 지정 workspace의 `s3-worker-fee0` 또는
`s3-worker-fee25` start/stop POST뿐이다. 빈 대상·restart·등록·다른 command는
IO 전에 거절하며 proxy/redirect, 응답 크기/시간, JSON과 오류 비노출을 제한한다.
POST는 실패해도 재시도하지 않는다. start timeout은 미실행 증거가 아니다.

이번 검증은 mock HTTP transport와 실제 session 조합이다. 실제 start/stop API
호출은 0회다. 서버 응답은 원 증거일 뿐 `operation.status` 성공이나 프로세스
종료를 인증하지 않는다. 설치 API는 `{workspace, operation}`을 반환하므로
다음 연결에서 operation 판정과 fresh runtime 상태·프로세스/포트/lock 종료
근거를 대조해야 한다. 등록 권한 정책은 그대로이며 인증 값은 설정/파일에
저장하지 않는다. 실행 가능한 orchestration CLI 및 전체 launcher는 미완성이다.

### 인증 session CLI (L-T 전용, L-R에서는 기동 금지)

먼저 `registration_cli.py packet`으로 생성한 exact command를 권한 있는
Paperclip 경로에서 등록한다. 현재 run의 인증 환경에서 다음 명령을 사용한다.
`WORKER_ARGS`는 packet에 넣은 `--approval-socket`, `--pid-mailbox`, 독립 승인
revision/판정 및 두 opt-in을 포함한 동일 argv 배열이다.

```sh
python3 -B ops/s3-local/session_cli.py run-reviewed \
  --python "$PYTHON_ABSOLUTE" --candidate "$CANDIDATE_ABSOLUTE" \
  --fee-bps 0 --workspace-id "$REGISTERED_WORKSPACE_ID" \
  --duration-seconds 120 -- "${WORKER_ARGS[@]}"
```

25bps는 별도 packet/workspace/home을 사용한다. 실행 시간은 정규 십진수
1–240초이며 등록된 worker 자체의 수명 상한도 그대로 적용된다.
현재 run 인증→후보 심사→capture 의미검증→관리 start→대기→stop→종료 관측을
연결한다. SIGINT/SIGTERM은 종료 요청으로 처리하며 승인 조회 중 신호는
start 전에 거절한다. 등록 API 쓰기나 임의 shell command 실행은 없다.

성공 stdout에는 종료 관측의 고정 boolean만 출력한다. 오류는 고정 문자열과
exit 2이며 인증 내용/응답 원문은 출력하지 않는다. mailbox는 자동 삭제·이동하지
않고 원 경로에 보존한다. 성공도 전체 descendant/cleanup 또는 DEV PASS를
인증하지 않는다. 이 CLI의 현재 검증은 mock session/제어 API 기반이다.

### 인증 fetch → C create (L-T 전용)

`python3 -B ops/s3-local/bootstrap_initialize_cli.py`는 `bootstrap_fetch_cli.py`의
필수 인자 전체에 `--home /absolute/new-engine-home`을 추가한다. 기존 인증
reader의 현재 run 환경을 사용한다. 이 명령은 승인 runtime에서 L-T가 실행한다.
L-R에서는 실제 RPC/create START를 실행하지 않는다.

먼저 새 home 경로와 승인을 확인하고 입력 capture를 고정한다. 한 번 조회한
원문을 `--evidence-root/snapshot-fetch.raw`에 no-replace/fsync로 보존한 뒤,
동일 메모리 bytes를 private create 사본에 전달한다. 파일을 다시 읽어 조회
출처로 신뢰하지 않는다. stage/READY/START 전 승인과 입력을 다시 비교한다.
C가 원문 의미를 검증하고 별도 원문 증거를 보존한다. 오류 후 자동 재시도,
home 삭제/수리/reseed는 없다. START 이후 오류의 생성 결과는 불명이므로
home과 증거를 보존해야 한다. 성공 report도 독립 replay/DEV PASS가 아니다.

이번 연결 검증: 합성 audit/fetch/stage/run 4개 및 실제 filesystem을 사용하는
기존 fetch CLI 회귀 5개 PASS. 실제 Rust/C 전체 조합·RPC·서비스는 NOT_RUN.

### Chain foreground 감독 (L-T 연결용 내부 API)

`chain_run.run`은 `chain_stage.stage` 수명 안에서 인증된 exact 후보 audit를
받는다. B preflight→READY→새 audit 뒤 START 직전에 다시 audit·사본 SHA·중단·
child 생존·handshake 기한을 확인한다. 정확한 `START\n` 한 번과 EOF만 보낸다.
foreground 수명은 기본 300초/최대 3600초, stdout+stderr 합계는 최대 1MiB다.
상한/오류/중단 뒤 process group kill·wait·pipe close를 수행하며 재시작하지 않는다.
출력 본문은 반환하지 않는다. START 이후 실패는 실행 결과 불명이고 home/원장/
증거를 삭제하거나 재초기화하지 않는다. `cleanup_complete_verified=false`는
별도 PID/port/lock 종료 관측이 필요함을 뜻한다. 이 내부 함수는 인증 reader·
Paperclip 관리 command 연결을 대체하지 않는다. L-R에서는 합성 subprocess로만
START를 시험했으며 실제 Chain START/DB/node/listener/RPC는 실행하지 않았다.

### Chain session 종료 관측

`chain_session.session`은 `prepare_chain`의 exact 단일 validator command와 `pid_mailbox.Mailbox`를 결합한다. trusted client/broker/audit를 전달하며 등록 API는 호출하지 않는다. stop operation 및 fresh workspace 대조 뒤 launcher/child PID와 해당 노드 RPC·P2P port만 관측한다. 실패/중단 시 mailbox를 보존한다. writer lock, 전체 descendants, 전체 topology, cleanup 인증은 별도이며 실제 관리 API orchestration/CLI 연결은 아직 남아 있다.

### Chain topology의 private preflight 입력

`chain_topology.preflight_payload(python, candidate, nodes, packet, staged,
fee_bps=...)`는 `prepare`의 exact packet을 다시 대조한 뒤 네 Go
`preflight` argv를 JSON bytes로 반환한다. 현재 staged scope의 입력/profile과
원 후보 capture를 대조하며 private 경로를 사용한다. 반환 bytes는
`nus-s3-local-chain topology --topology <private-json>`의 입력이다.
파일 게시/child 감독/실제 B topology 성공 종단 연결은 아직 남아 있다.
이 함수는 home identity·포트 가용성·조직 승인·기동 허가를 판정하지 않는다.

### Chain topology private preflight 감독

`chain_topology_check.check(python, candidate, nodes, packet, staged,
fee_bps=..., scratch=...)`는 살아 있는 `chain_stage.stage` 범위에서 호출한다.
관리 packet 대조 후 네 preflight argv를 private root0700/topology.json0600에
no-replace로 기록하고 file/root fsync를 마친다. 기존 bounded 감독으로
`nus-s3-local-chain topology --topology <private-file>`만 실행하며 stdin은 EOF다.
종료 전후 staged bytes와 topology 원문을 재검사하고 exact node ID 응답만 허용한다.
시간/출력 상한·환경 격리·kill/reap과 임시 파일 정리를 공유한다.
이 API는 조직 승인·포트 가용성·writer exclusion이나 서비스 실행 허가가 아니다.
이번 신규 시험은 합성 subprocess이며 실제 B topology 성공 종단은 NOT_RUN이다.

### 웹 fault 등록 packet 출력

`python3 -B ops/s3-local/web_registration_cli.py web-packet --python /absolute/python3 --candidate /absolute/candidate --fee-bps 0 -- <serve-web-reviewed 뒤의 exact 인자>`는 등록 요청 JSON만 stdout에 출력한다. fee25는 `--fee-bps 25`를 사용한다. API 등록·mailbox 생성·서비스 기동은 수행하지 않는다. host command 등록에는 기존 권한 경로가 필요하다.

기본 fault는 비활성이다. 응답 유실 시험은 웹 인자에 `--drop-broadcast-response-sha256 <exact HTTP body SHA256>`를 명시한다. 두 opt-in 및 승인 참조는 그대로 필수다. fault hash도 등록 command 원문의 일부이므로 선택/변경 후 이전 등록과 일치한다고 취급하지 않는다. L-T에서 승인된 후보와 exact 등록을 확인한 뒤 사용한다. 응답 유실 보고서의 영속 게시 연결은 아직 남아 있다.

### 저장 fault의 인증 READY 검사

`python3 -B ops/s3-local/storage_fault_cli.py check-ready-reviewed`에
`reviewed_cli.py`와 같은 bundle/artifacts/input-set/effective-profile/home/key-directory/
scratch/runtime-pin/local-demo-profile/bind/rpc/lifetime-seconds/max-requests/max-ticks,
`--acknowledge-unproven-space`, native-decision-id/ceo-revision/cto-revision을 전달한다.
추가 필수 인자는 `--enable-storage-fault --fault-point before_wal --fault-occurrence 1
--fault-purpose NORMAL --fault-evidence-root /절대/새/증거경로`다.
현재 run의 인증 reader를 사용하고 전용 descriptor→C validator→private child READY→
새 승인 조회를 수행한다. START는 보내지 않으며 RPC나 fault 증거 root를 생성하지 않는다.
child 종료와 사본 정리가 끝난 뒤에만 JSON을 출력한다. 이 보고서는 재사용 허가가 아니다.
실제 fault 실행 명령과 L-T의 장애 검증은 별도이며 아직 미완료다.

### 저장 fault errno 선택

READY 검사와 L-T의 `run-reviewed`에 선택 인자
`--fault-errno ENOSPC`, `--fault-errno EDQUOT`, `--fault-errno EIO`를 전달할 수 있다.
생략하면 기존 Generic/v2 경로다. errno 선택은 전용 child의 exact argv로 전달되고
기존 Worker Seal API에서 명령 원문·v3 보고서에 결합된다. host 공간/쿼터를 변경하지 않는다.
알 수 없는 값·중복 옵션은 거절한다. child 결과의 injected/command_succeeded는 별개이며
errno와 명령 SHA의 상세 근거는 영속 fault 보고서에서 확인한다.
이번 연결 검증은 컴파일·옵션/감독 시험·실제 child의 기동 전 거절까지다.
errno를 선택한 실제 child의 START/RPC/Seal 종단은 L-T에서 수행해야 한다.

## 최신 component 소스 포함 검사

`python3 -B ops/s3-local/component_sources.py --source . --candidate <full-40-hex-commit>`은 기록된 B/C/L-D/L-E head의 `chain/`, `exchange/`, `settlement/`, `web/` 파일을 지정된 Git commit과 대조한다. 원 파일 SHA256·mode·누락 및 새 파일 목록을 출력한다. ancestor인 원본을 나중에 덮어쓴 후보도 거절(exit1)하며, 추가 SRE 파일은 별도 심사 대상으로 남긴다. symlink/submodule·축약 ref·빈 component는 거절한다.

검사는 commit object만 읽는다. 미커밋 작업 파일은 포함하지 않으며 `working_tree_checked=false`를 표시한다. 최신 문서 revision/철회/판정·A 계약 집합·component 밖 파일·실제 build 포함·실행 파일·완전한 source inventory는 별도 대조해야 한다. 따라서 `approval_verified=false`, `runtime_approved=false`, DEV NOT_RUN이다. 이 보고서는 runtime manifest 집계에 넣지 않는다. 현재 HEAD의 실패를 이미 옮겨 둔 미커밋 Wallet 파일의 부재로 단정하지 않는다.

### Manifest 원문 읽기의 경합 검사

`manifest.read_regular`은 경로 사전 검사 뒤에도 directory fd와
`O_NOFOLLOW`/`O_NONBLOCK`으로 연다. 단일 regular inode·512 MiB 이하
(runtime byte preflight와 같은 상한)를 확인하고, 최초 크기까지만 읽으며
읽기 전후 inode/mode/nlink/size/mtime/ctime 및 마지막 경로 inode를 대조한다.
읽기 중 변경은 후보 반환 전에 거절한다. 이 검사는 지속 잠금이나 전체
상위 경로의 불변 보장이 아니며 마지막 source/artifact 재검사와 실행 capture,
CEO/CTO 독립 승인·CTO→Security gate를 대체하지 않는다.

### 별도 crash child 준비

`runtime/crash_main.rs`는 fault-injection build 전용입니다. 인자는
`crash-seal-captured --start-gate-fd FD --capture-sha256 SHA
--enable-storage-crash --fault-point POINT --fault-occurrence N
--fault-purpose NORMAL|RESOLVE_FAILURE --fault-evidence-root ABS
--worker-inputs ...` 순서입니다. worker 입력의 두 개발 opt-in도 필수입니다.
기존 writer/private signer 준비 후 READY, 인증 부모의 START 이후에만
관측 1회와 기록된 Seal crash API를 호출합니다. 관측 저장은 crash 예약보다
앞섭니다. hook 도달 시 exit86으로 Drop 없이 종료하며 예약만으로 crash 성공을
판정하지 않습니다. 정상 반환은 hook 미도달이며 명령 성공 여부를 별도로 표시합니다.
현재 컴파일/잘못된 CLI 거절만 검증했습니다. descriptor/private 사본·인증 부모의
실제 READY 연결은 남아 있으며 L-R에서 START/RPC/서비스를 실행하지 않습니다.

### Crash private 실행 사본 준비

`storage_crash_stage.stage`는 SRE descriptor의 전용 `bin/s3-local-storage-crash` SHA만 사용한다.
일반 worker나 IO fault binary로 fallback하지 않는다. 초기 승인 검사 뒤 같은 capture를 C validator에 전달하고,
root0700/binary0500 사본의 inode·권한·SHA와 새 승인 조회를 확인한다.
반환 객체는 재사용 허가가 아니며 READY/시작 직전 인증 부모 연결은 아직 남아 있다.
정상/오류/interrupt 종료 시 임시 실행 사본만 제거하며 home·fault 증거는 보존한다.
이번 검증은 합성 reader/validator/바이트의 준비 경계 시험으로 실제 crash child 실행0이다.

### Crash READY 감독

`storage_crash_ready.ready`는 `storage_crash_stage.stage` 안에서 동일 worker 인자와 인증 audit를 받아 사용한다. 전용 crash opt-in과 `--worker-inputs`를 생성하고 IO/errno 옵션 혼합을 거절한다. READY 뒤 승인/bytes를 다시 확인하며 START 없이 child를 kill/reap한다. 합성 subprocess 검증과 실제 Rust/C READY 연결을 검증했다. fee0/25에서 실제 descriptor SHA→capture→C validator→private crash child READY 뒤 승인 철회/stop/interrupt를 거절하고, START0·증거 root 생성0·private 사본 정리·writer 재개방·두 번 replay/commit 불변을 확인했다. 조직 승인 reader/descriptor/pin/키는 합성이며 실제 crash 실행/서비스/DEV 승인이 아니다. 근거: NUS-73 crash-ready-real 시험 기록.

### Crash 인증 READY 검사 CLI

`python3 -B storage_crash_cli.py check-ready-reviewed`에 `offline_cli`의 reviewed 입력
(bundle/artifacts/input-set/scratch/runtime-pin, CEO/CTO revision, native decision,
worker 인자와 두 개발 opt-in) 및 `--enable-storage-crash --fault-point POINT
--fault-occurrence N --fault-purpose NORMAL|RESOLVE_FAILURE --fault-evidence-root ABS`를 전달한다.
현재 run의 인증 reader→전용 private 사본/C validator→READY 뒤 새 audit를 연결한다.
START를 보내지 않으며 `run-reviewed`, errno/Apply 옵션, 중복/축약 인자는 거절한다.
child reap과 사본 정리·signal 복원이 끝난 뒤에만 JSON을 출력한다.
오류는 exit2/고정 진단/stdout0이며 READY 성공은 crash 또는 runtime 승인이 아니다.
CLI mock 5시험과 기존 READY4/stage5 회귀14 PASS. 실제 Rust/C 재시험은 이번에 하지 않았다.

crash 실행 부모의 실제 child 최종 gate 시험은 `test_real_storage_crash.py`를 사용한다. 세 번째 audit에서 승인 철회/capture 변경/stop/interrupt를 주입하며 START 없이 거절·원 입력 보존·증거 root 미생성·사본 정리를 검사한다. 상위 Rust fixture는 fee0/25 writer 재개방·두 번 replay/commit 불변을 검사한다. 승인 reader/descriptor/pin/키는 합성이며 실제 crash 실행 또는 DEV 판정이 아니다.

### Apply crash 명령 선택

`storage_crash_cli.py check-ready-reviewed` 또는 `run-reviewed`의 기존 Seal 인자에
`--fault-command Apply`를 명시하면 `crash-apply-captured` child와
`s3-local-crash-apply-result/1` 응답을 사용한다. 생략하면 Seal이다.
Apply는 `--fault-purpose NORMAL`만 허용하며 errno와 혼합하지 않는다.
READY 검사는 START를 보내지 않는다. 실행 명령은 승인 pin 이후 L-T 범위다.
exit86은 UNKNOWN이며 보고서/replay 검증을 대신하지 않는다.
이번 배선 검증은 mock CLI·합성 subprocess 및 실제 binary 입력 거절이다.
유효 Apply crash child의 READY/START/RPC 종단은 아직 NOT_RUN이다.

### F05 인증 READY 검사

`python3 -B ops/s3-local/before_send_cli.py check-ready-reviewed`에 기존
`offline_cli`의 bundle/artifacts/runtime-pin/input-set/scratch 및 worker 입력,
`--native-decision-id`, `--ceo-revision`, `--cto-revision`을 전달한다.
추가 필수 인자는 `--enable-f05-before-send --tx-hash <64자리 소문자 hex>
--fault-evidence-root <새 절대 경로>`다. 기존 두 opt-in도 모두 필요하다.
전용 descriptor/private binary와 C 의미 검증 뒤 READY를 확인하고 승인을
재조회한다. child reap과 private 사본 정리 뒤에만 결과를 출력한다.
READY 검사 자체는 START/방송/증거 root 생성을 수행하지 않는다.
READY 성공은 F05 주입 또는 재사용 실행 허가가 아니다.
`before_send_run.run`/`before_send_cli.py run-reviewed`는 READY 뒤 세 번째
승인·실행 바이트·stop/기한을 재확인하고 정확한 START+EOF를 한 번만 보낸다.
exit86·부분/누락 보고는 UNKNOWN이며 transport 호출, F05 충족, crash/replay
검증으로 승격하지 않는다. 시작 전 철회/변조/중단은 START0, 시작 뒤
오류/출력/시간 초과는 결과 불명으로 child를 정리한다.
