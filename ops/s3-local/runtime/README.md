# L-R HTTP 전송 경계 — 기동 전 구현

[NUS-73](/NUS/issues/NUS-73)의 SRE 소유 배선이다. 승인 L-D `Rest`·C·경제 로직·protocol·Cargo lock을 변경하지 않는다. **listener·worker 실행 파일은 아직 없으며 서비스 시작 명령이 아니다.**

- `http.rs`: literal loopback bind/실제 peer·정확한 Host, 단일 HTTP/1.1 요청, body 16384 bytes·헤더32개·각 line4096 bytes(CRLF 포함), 전체 전송 deadline2초. 본문 할당 전에 Content-Length 상한을 검사한다. DNS/forwarded header로 peer를 대체하지 않는다.
- Content-Length 중복·Transfer-Encoding 전체·Expect/upgrade·obs-fold/control·비정규 길이·absolute target·query/fragment/escaped path·본문 있는 GET/OPTIONS를 거절한다. POST는 application/json과 정확한 Content-Length를 요구한다. auth/origin 등의 원 헤더 중복은 handler로 보존한다.
- 한 요청만 dispatch하고 socket을 닫는다. 두 번째 pipelined 요청은 처리하지 않는다. 응답은 no-store·JSON·nosniff·Connection close이며 exact 두 origin 외 값을 반사하지 않는다. CORS OPTIONS는 승인 origin/GET 또는 POST/authorization 및 content-type에만 허용하고 handler·인증·effect를 호출하지 않는다.
- `rest.rs`: 승인 L-D `Rest::handle`에만 전달한다. peer는 socket.peer_addr, bind는 socket.local_addr이다. owner/관측은 browser에서 가져오지 않는다. now는 요청을 읽은 뒤 실제 Unix ms를 얻고, 외부 trusted adapter가 준 Observation의 원 시각을 보존한다. stale 관측 갱신/서명/방송/경제 전이 우회 없음.
- 새 package/의존성/feature 없음. 현재 코드는 Cargo 표준 build에 포함하지 않는다. 최종 실행 파일 배선 때 이 모듈과 명령·SHA를 SRE descriptor에 포함해야 한다.

## 최소 검증

설치된 Rust1.92.0에서 `rustc --edition=2024 --test ops/s3-local/runtime/http_test.rs -o "$PAPERCLIP_RUN_SCRATCH_DIR/http-test"` 뒤 생성된 시험 실행 파일을 `--test-threads=1`로 실행한다. 메모리 Wire만 쓰므로 listener/RPC 없음. 14 PASS/0 FAIL: framing·입력 상한·peer/host·deadline·pipeline·CORS·중복 보존·거절 전 effect0. 실제 TCP deadline·통합 인증·체인·DEV05/12 PASS의 근거가 아니다.

`rest.rs`는 승인 L-D를 포함하는 기존 `--offline --locked --features dev-local-settlement` build의 rlib와 serde_json rlib로 `rustc --edition=2024 --crate-type lib --extern … -L dependency=…` 컴파일했다. 이는 API 타입 연결 확인이며 실제 listener/HTTP 통합 실행이 아니다.

남은 실행 배선: bounded accept/process 종료, 신뢰 Chain query·receipt/block/results adapter, worker loop와 비공개 operator signer, 웹 ChainPort/mount, 새 fee0/25 home/genesis·launcher/preflight/cleanup/fault driver, 전체 실제 binary/web의 최종 manifest·독립 pin 출처·CTO→Security. 서비스0·pin 미발급·DEV NOT_RUN·G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED 유지.

## 신뢰 로컬 조회 수집 (추가 진행)

`query.rs`는 읽기 전용 ABCI 6개 경로와 exact-height block/block_results, hash-bound tx만 허용한다. 요청은 Comet v0.40.0의 로컬 설치 소스(`rpc/core/abci.go`, `rpc/core/tx.go`, `libs/bytes/bytes.go`, `libs/json/decoder.go`)와 대조했다: ABCI data는 HexBytes, Tx hash는 base64 bytes다. 서명·broadcast API는 없다.

literal loopback:1024..65535에 직접 연결하고 전체 connect/write/read 2초 deadline, 헤더16KiB/64개·line4096, body16MiB(상속 C RPC evidence cap), wire 추가256KiB·chunk32768개를 상한으로 둔다. 고정 Content-Length 또는 chunked만 받으며 중복 길이·CL/TE 혼용·압축·trailer·redirect·절단·후행 bytes를 거절한다. 원 JSON entity bytes는 재직렬화하지 않는다. HTTP200/JSON-RPC 오류·NOT_FOUND는 확정 또는 실패 증명으로 승격하지 않는다. 실제 TCP deadline/Comet 응답 호환성은 L-T 시험에 남는다.

`collect.rs`는 승인 L-D `decode_snapshot`으로 Context·높이·원 snapshot을 검증하고, 동일 snapshot 높이의 block/results 원문을 C `Objects`에 보존해 기존 `proof::block`을 호출한다. 이것만으로 terminal attempt·absence proof를 만들지 않는다. 연속 snapshot 적용·freshness/Observation·receipt/ConfirmedTx·worker 전이는 후속 배선이다. 기존 C/L-D 소스와 lock 변경0이다.

기존 Rust1.92.0 및 L-D feature rlib로 다음 순수 검증을 실행했다. `DEPS`는 이미 `--offline --locked --features dev-local-settlement`로 만들어진 dependency 디렉터리, 각 `*_RLIB`는 그 디렉터리의 실제 rlib 파일이다. 새 설치나 cargo 해석 없이 그대로 연결한다.

```sh
rustc --edition=2024 --test ops/s3-local/runtime/collect.rs \
  --extern nus_exchange_contract="$NUS_RLIB" --extern serde_json="$JSON_RLIB" \
  --extern base64="$BASE64_RLIB" --extern hex="$HEX_RLIB" \
  -L dependency="$DEPS" -o "$PAPERCLIP_RUN_SCRATCH_DIR/collect-tests"
"$PAPERCLIP_RUN_SCRATCH_DIR/collect-tests" --test-threads=1
```

**13 PASS / 0 FAIL**, query10개를 포함한 총수이며 별도 실행 query10개와 합산하지 않는다. 메모리 fixture만 사용, socket/RPC0·서비스0. 블록 높이/hash/chain/count 불일치·중복 JSON·RPC 오류 거절과 원 증거 byte 보존을 확인했다. 합성 block fixture는 runtime pin이나 실제 체인 증거가 아니다. 최종 manifest·독립 승인·DEV 통합은 여전히 미완료다.

## 연결 수락·종료 루프 (추가 진행)

`lifecycle.rs`와 `rest.rs::serve_service`는 이미 검증·bind된 listener를 승인 REST에 연결한다. exact `127.0.0.1:1024..65535` 대조 후 nonblocking accept, accepted stream blocking 설정, 동시 연결1개·추가 thread/사용자 공간 queue0이다. 각 반복은25ms 쉬고 trusted tick을 먼저 실행한다. tick 오류는 즉시 루프를 종료하며 재시도·새 Observation 발급·자동 정정을 하지 않는다. `serve_service`는 성공한 trusted callback의 관측 시각을 변경 없이 보존한다.

수명은1..3600초 범위(0초 초과의 Duration), 요청 상한은1..100000이며 거절된 요청도 포함한다. 정상 반환에는 종료 원인·성공/거절 요청 수·tick 수가 남고 오류는 상위 launcher의 실패 종료로 전달해야 한다. 정지 AtomicBool 또는 monotonic 수명 상한에서 신규 요청을 닫으며 accept 중 정지해도 연결을 처리하지 않는다. 종료 시 listener/연결을 drop하고 home·key·WAL·guard·lock inode는 삭제하지 않는다. Engine 소유자는 반환 후 drop하여 writer lock을 해제해야 한다.

이는 프로세스 hard timeout이나 CPU/메모리 RLIMIT 구현이 아니다. 실행 중 callback은 강제로 중단하지 않으므로 종료 지연은 현재 callback 시간만큼 늘어날 수 있다. HTTP는 기존2초 deadline을 사용하지만 trusted tick의 전체 RPC 호출 수·deadline, OS signal 연결·관리 runtime의 강제 종료 grace는 최종 executable/launcher에서 추가로 고정해야 한다. 이 루프 자체는 bind·키 생성·서비스 시작을 하지 않는다. 실제 listener·신호·포트 해제는 L-T 검증이다.

기존 `/Users/gangdongju/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin/rustc`로 다음을 실행했다. 주입 HOME의 rustup shim은 쓰기 거절됐으며 새 설치 없이 설치된 binary 절대 경로로 해결했다.

```sh
rustc --edition=2024 --test ops/s3-local/runtime/lifecycle.rs -o "$PAPERCLIP_RUN_SCRATCH_DIR/lifecycle-tests"
"$PAPERCLIP_RUN_SCRATCH_DIR/lifecycle-tests" --test-threads=1
```

**9 PASS / 0 FAIL**: 잘못된 상한·endpoint, 시작 전 정지, worker 오류 뒤 accept/retry0, tick 선행·거절 요청 계수, idle 수명·catch-up 방지, listener 실패, accept 중 정지/drop, tick 후 수명 만료를 가상 시계/가짜 연결로 확인했다. Rust 표준 라이브러리만 사용하고 socket/RPC0이다. 승인 L-D feature의 기존 rlib와 `rest.rs` library 컴파일도 PASS. 전체 executable, 실제 worker/proof/signer, ChainPort, fee0/25 초기화·launcher·최종 manifest는 여전히 미완료다.

## 순차 관측과 C 저장 연결 (추가 진행)

`observe.rs::Observer`는 승인 bootstrap binding으로 decode한 마지막 저장 관측과 C의 `latest_observation_ref`(없으면 `chain_snapshot`)·Context를 비교한다. 적용 원장보다 앞선 관측이 저장돼 있으면 bootstrap으로 되돌아가 시작하는 것을 거절한다. `tick`은 읽기 전용 `ChainRead`와 승인 `Worker::reconcile(Command::Snapshot)`만 호출한다. REST/browser가 snapshot·시각·높이를 지정하는 경로가 아니다.

한 tick은 latest 조회1개, gap이면 exact next-height 조회1개, 저장 최대1개로 제한한다. `Snapshot::advance`의 연속 높이·epoch·계정·terminal slot 검증을 그대로 사용한다. gap을 한 번에 건너뛰거나 반복 조회하지 않는다. 원 ABCI 응답 bytes는 RPC evidence로 C에 전달하고 C 저장 성공 뒤에만 내부 anchor와 Observation을 공개한다. 어떤 조회/시각/검증/저장 오류나 unwind든 lane을 닫으며 다음 호출은 IO 전에 거절한다. 같은 높이의 동일 snapshot은 재저장하지 않는다.

Observation의 received_at은 전체 조회 뒤·저장 전 wall clock, query_latency_ms는 tick 시작부터 조회가 끝날 때까지 monotonic elapsed다. 저장 후 시각을 덮어쓰지 않는다. gap은 catching_up=true이고 오래된 블록/느린 조회는 기존 C/REST freshness 검사에서 접수가 제한된다. 이미 저장할 수 있는 과거 관측은 대사를 위해 저장하되 freshness를 위조하지 않는다. 네트워크 예산은 최대2회×2초, C fsync의 OS 차단 시간은 별도이며 hard process timeout을 주장하지 않는다.

설치 Rust1.92.0과 기존 offline/locked L-D rlib로 **20 PASS / 0 FAIL**(observer7+상속 query/collect13)을 확인했다. 새 observer7개 중 실제 C store 시험1개는 fee0/25 각각 Snapshot 저장→CATCHING_UP·미적용 C 보존→close→두 번 replay의 동일 state/commit 및 마지막 관측 anchor 복구를 검증했다. 나머지는 메모리 callback으로 저장 실패·영구 닫힘·조회 상한·원 bytes·시각 보존을 확인했다. RPC/listener/서비스0이다. 최초 store 시험은 시험 RPC를 정수 금지 canonical encoder에 넣어 NON_CANONICAL_VALUE로 실패했으며 RPC 원문 생성만 serde_json으로 수정한 뒤 통과했다. 경제/체인 구현 변경0이다.

재현은 `observe-build.json`의 rustc argv와 `CARGO_MANIFEST_DIR=<checkout>/exchange`를 사용한다. `--test ops/s3-local/runtime/observe.rs`와 기존 `nus_exchange_contract/serde_json/base64/hex/fips204` rlib, `-L dependency=...`가 필요하다. 시험 실행에는 `PAPERCLIP_RUN_SCRATCH_DIR`을 지정한다. 시험 home/genesis/key/pin은 공개 합성 fixture이며 서비스에 사용할 수 없다.

잔여: terminal attempt/receipt proof·signer·seal/submit/apply worker loop, 웹 ChainPort/mount, fee0/25 실제 초기화, launcher/cleanup/fault driver, 최종 binary·manifest·독립 승인 출처. 이 observer는 확정 실패/영수증/Apply를 임의로 만들지 않는다. 서비스0·pin 미발급·DEV NOT_RUN 및 원 G00/ACK 상태를 유지한다.
