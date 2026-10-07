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

## 확정 TX 포함 수집 (추가 진행)

`ChainRead::confirmed(snapshot, persisted_tx)`는 검증된 단일 snapshot H의 block/results 두 응답을 가져와 exact TX bytes의 위치를 찾고, 원 RPC 두 개와 TX를 Objects에 보존한 뒤 승인 C `proof::confirmed`를 호출한다. snapshot H/hash·chain·TX index/hash·code/codespace/gas를 기존 검증기로 대조한다. code/gas의 RPC 정수/문자열 표현만 계약의 정수 문자열로 옮기며 음수·소수·비정규 정수·u32 code 초과는 거절한다. 동일 TX가 여러 위치에 있으면 모호한 위치를 선택하지 않고 거절한다.

조회는 최대2회×기존2초 제한이며 빈/초과 TX를 IO 전에 거절한다. 미발견은 `None`이고 단일 블록에 없다는 뜻뿐이다. timeout 전체 부재 증명·terminal attempt 전이·VOID/정정·D/P 해제·receipt COMMITTED를 생성하지 않는다. nonzero ABCI code도 포함 증거의 메타데이터일 뿐이다. 반환 Objects는 메모리 evidence이며 worker가 C API로 저장해야 영속 증거가 된다. RPC 실패는 오류로 전파하며 재시도/추정은 하지 않는다.

기존 offline/locked rlib와 Rust1.92.0으로 `--test ops/s3-local/runtime/collect.rs`를 빌드하여 **20 PASS / 0 FAIL**(신규 포함 시험7 + 기존 query/collect13)을 확인했다. exact 두 번째 TX/index·raw bytes 보존, 성공/실패 code, 빈 블록/미발견, 중복 TX, 잘못된 정수·범위, snapshot/hash/raw-ref 변조, TX 크기 거절을 메모리 fixture로 검증했다. 실제 socket/RPC/서비스0이며 기존 observer 시험20과 합산하지 않는다. 컴파일 명령은 `inclusion-build.json`, 원 결과는 `inclusion-tests.log`다.

잔여 worker/receipt/absence/signer·ChainPort·초기화·launcher·최종 manifest·독립 승인 범위와 DEV NOT_RUN은 유지한다.

## Batch 조회와 timeout 전체 부재 수집 (추가 진행)

`ChainRead::batch`는 exact H의 B Batch 조회 원문과 `BatchLookup`을 반환한다. JSON-RPC id/error/code, canonical value·Context·H·snapshot id·requested seq·LastBatch를 검증한다. seq>LastBatch일 때만 NOT_FOUND_AT_HEIGHT/null을 허용하고, 이미 지난 seq의 누락은 RECEIPT_INCONSISTENCY다. FOUND는 조회 자료일 뿐이며 terminal TX 검증 전 COMMITTED가 아니다.

`ChainRead::absence`는 C에 저장된 Attempt와 정확히8개 연속 snapshot을 받는다. timeout 이후 높이·같은 Context·계정 번호/sequence를 확인한 다음 Batch 조회1회, block/results16회까지만 수행한다. 각 RPC deadline2초, 원 증거 합계16MiB이며 C `proof::absence`가 TX 미포함·연속 block hash를 검증한다. 조회 오류는 재시도 없이 전파하고 partial proof/상태 전이를 반환하지 않는다. 반환은 메모리 proof와 원 evidence이며 영속화·Attempt 전이·봉투 재시도·정정은 수행하지 않는다. NOT_FOUND 단독으로 D/P를 해제하지 않는다.

**운영자 Account 연결:** `account.rs`와 `ChainRead::account`는 정확히 snapshot H의 `/cosmos.auth.v1beta1.Query/Account` 원문을 검증한다. 승인 SDK v0.55.0의 QueryAccountResponse/Any/BaseAccount와 ML-DSA Any만 허용한다. 주소·키 해시·H·RPC id/code, protobuf 중복/미지 필드·잘못된 wire·비최소 정수·overflow·잘림을 거절한다. 등록 사용자라면 snapshot의 계정 번호/sequence/키와도 같아야 한다. SDK가 생략한 0 scalar는 0으로 해석하고 u64 정수는 손실 없이 보존한다.

`Account` 내부 필드는 private이다. `absence_with_account`는 검증된 Account의 snapshot id/owner/number/sequence를 IO 전에 대조하고 원 응답을 evidence에 추가한다. 등록되지 않은 운영자도 이 경로를 사용할 수 있으며 snapshot에 임의 사용자 추가는 하지 않는다. 기존 `absence`는 snapshot-only 경로여서 계정 누락을 계속 거절한다. Account는 신뢰 로컬 RPC 관측이며 Merkle 증명이 아니다. 서명·방송·영속화·worker 연결은 아직 남아 있다.

Rust1.92.0·기존 offline/locked rlib로 **28 PASS/0 FAIL**(신규8+기존20), 실제 socket/RPC/서비스0. 8개 높이·17개 원 evidence 보존, 포함TX/깨진 hash chain·과거 seq 영수증 누락·잘못된 Context/H/id/정수·원문 canonical 위반·history gap·timeout equality·계정 누락·sequence 역행·총량초과·RPC 중단을 시험했다. 최초4개 실패는 fixture에서 운영자 계정이 없었던 것에 따른 panic이며 실패 로그를 보존하고 등록 운영자 fixture와 미등록 거절시험을 분리했다. 경제/Chain/C/lock 변경0. `absence-build.json` 명령과 `absence-tests.log`가 재현 근거다.

다음 SRE 작업: receipt/signer/worker, 웹 ChainPort, fee0/25 초기화·launcher/정리/fault driver·최종 binary/manifest. CTO→Security 제출 전이며 runtime pin 미발급·DEV NOT_RUN·원 G00/ACK와 부모 blocker 유지.


Account 연결 검증: 기존 시험과 신규7개를 합쳐 **35 PASS/0 FAIL**. SDK v0.55.0 실제 Marshal 결과를 `testdata/account-sdk.go`로 offline 생성하고 Rust decoder와 대조했다. 이 generator는 합성 공개키만 쓰며 서비스/서명/방송을 실행하지 않는다. `account-build.json`·`account-sdk-build.json`과 로그가 명령 근거다. generator 첫 컴파일의 any 이름 충돌과 fixture 주소 hex 직렬화 오류는 수정했고 최초 실패 로그를 보존했다. 실제 RPC·worker·DEV 통합은 NOT_RUN이다.

## COMMITTED receipt 수집

`ChainRead::committed_receipt`는 신뢰하는 current/terminal Snapshot과 영속 BatchIdentity·TX 원문을 받는다. 같은 current H Batch 원문을 검증하고 terminal H의 exact TX/index/block/results를 기존 C `proof::receipt`로 결합한다. 원 Batch 응답도 Objects에 보존한다. Context/Batch/terminal H/hash 불일치는 block IO 전에 거절한다. nonzero code·TX 누락·위조 receipt wire·VOID·NOT_FOUND·조회 오류는 COMMITTED로 승격하지 않는다. 새 경제 로직/엔진 전이/서명/방송은 없다. VOID resolution evidence와 worker 영속 적용은 후속 배선이다.

합성 순수시험 신규6개와 기존35개 총41 PASS. 실제 RPC/서비스0, DEV NOT_RUN, runtime pin 미발급. 원 G00/ACK·부모 blocker 유지.

## VOID receipt 전송 연결

`ChainRead::void_receipt`는 C가 영속화한 `ResolutionEvidence` reference와 Objects를 받는다. 매번 원 bytes·schema·Context·Batch를 대조하고 참조 closure만 복사한다(원 증거 closure 16MiB 상한). 같은 H Batch의 실패 TX hash·resolution domain hash·terminal H/TX를 대조한 뒤 실제 성공 CLOSE TX 포함 증거를 조립한다. 원 Batch RPC·block/results·TX·failure evidence를 함께 반환한다. hash만 있고 원 증거가 없거나 참조가 변조되면 IO 전에 거절한다.

이 반환은 **전송 증거 조립**이며 실패 또는 정정 승인이 아니다. 승인 C `record_receipt`가 저장된 failure_evidence·CLOSE attempt·CLOSING·모든 settle attempt 종결과 정확한 증거를 다시 확인해야 한다. 수집기는 Command/Apply나 경제 전이를 실행하지 않는다. 그 연결은 worker 구현에 남는다. 기존 C/L-D 및 lock 변경0.

기존 offline/locked rlib와 Rust1.92.0으로 `collect.rs` 순수시험 **46 PASS / 0 FAIL**(신규 VOID 5개 + 기존41개). VOID 양성 fixture는 전송 검증용 합성 schema이며 엔진이 승인한 실패 증거가 아니다. 원 bytes 보존·hash-only/변조 거절·lookup 불일치·실패/누락 CLOSE TX 거절을 확인했다. 실제 RPC/서비스0·DEV NOT_RUN·runtime pin 미발급. worker/signer·웹 ChainPort·초기화/launcher·최종 manifest 및 CTO→Security는 미완료다.

## 비공개 operator signer

`signer.rs::LocalSigner`는 별도 canonical 절대 key 디렉터리(root0700·현재 euid) 아래 `operator.seed` 32 bytes만 읽는다. directory FD에 상대적인 openat·NOFOLLOW/CLOEXEC/NONBLOCK, regular file·현재 euid·0600·nlink1·정확한 길이·읽기 전후 metadata를 검사한다. seed는 Zeroizing으로 지우고 fips204 PrivateKey의 ZeroizeOnDrop을 사용한다. 같은 uid의 악성 프로세스나 메모리/core dump에 대한 격리 보장은 아니다. 새 키 생성·실서비스 seed 사용·복구·보관 구현은 이 모듈에 없다.

로드 시 pinned genesis/operator 구성에서 얻은 예상 공개키와 ML-DSA-65 파생 공개키가 일치해야 한다. 기대키를 REST/browser에서 받으면 안 된다. `OperatorSigner`를 구현하며 L-D가 조립한 bounded SignDoc에 빈 context와 FIPS 204 deterministic signing(rnd=0)을 사용한다. signer 자체가 SignDoc 경제 의미를 다시 구현하지 않는다. 승인 L-D는 결과 서명·operator address/key를 검증하며 영속 intent 뒤에만 방송한다. runtime 내부에서만 접근하고 HTTP API로 노출하지 않는다.

기존 offline/locked cache의 fips204 0.4.6·libc·zeroize rlib를 사용했다. Cargo.toml/lock 수정0이며 최종 rustc build에서 이 세 rlib도 exact 명령·SHA 입력으로 기록해야 한다. 순수시험 **7 PASS / 0 FAIL**: 합성 키 실제 서명 검증·문서 상한, root/file 권한, 길이, 공개키 mismatch, symlink/hardlink, FIFO/디렉터리 거절. 시험 파일은 run scratch에서 생성·정리하며 공개 seed는 runtime용이 아니다. 최초 trait 오류 타입 컴파일 실패 후 C Error 타입으로 수정해 통과했다. 서비스/RPC0, DEV NOT_RUN. signer와 실제 worker의 통합은 후속 배선이다.

## Account를 결합한 제출 lane

`submit.rs::SubmitLane`은 동일 Engine으로 생성한 승인 L-D Worker에만 준비/방송을 위임한다. prepare는 C의 마지막 저장 관측·Context·freshness 확인 → 같은 H 운영자 Account 조회1회 → private Account binding 대조 → 원 Observation 시각으로 freshness 재검사 → `Worker::prepare_settle` 순서다. 조회 후 시각을 새 Observation으로 발급하지 않는다. 조회 중 stale·다른 snapshot·RPC 오류는 signer와 commit 전에 거절한다. L-D가 미해소 시도와 재시도 예산을 다시 검사한다. 반환 account_rpc는 audit 원문이며 C에 저장한 terminal proof를 뜻하지 않는다.

방송은 별도 `broadcast_existing`에서 저장된 exact TX hash만 받는다. 같은 Context/operator/epoch 및 timeout 이전인지 검사하고 L-D의 영속 UNKNOWN/count → writer gate 내 bounded IO 순서를 사용한다. 이 경로는 signer·새 TX·새 Batch를 만들지 않는다. 두 메서드는 오류/unwind 후 lane을 닫으며 restart는 C 저장 상태를 다시 읽어야 한다. 조회1회는 기존2초 한도, 방송은 L-D의 최대2초 지연+2초 IO 한도이며 OS fsync 차단시간은 별도다. 자동 seal/terminal proof/Apply·반복 scheduling은 후속 worker 조립에 남는다.

검증 결과와 exact rustc 명령은 `submit-build.json`·`submit-tests.log`로 인계한다. 실제 C store의 fee0/25 준비·원 TX/횟수 보존·미해소 TX 재서명0·두 번 replay, 조회 실패 후 IO 재시도0, 조회 중 stale, 잘못된 Account/저장 snapshot의 서명0을 순수시험한다. 실제 방송·listener·서비스는 실행하지 않으며 해당 연결의 실제 검증은 L-T에 남는다. engine/Chain/경제/lock 변경0이다.

## 포함 증거의 영속 Attempt 전이

`SubmitLane::resolve_inclusion`은 신뢰 관측의 한 높이에 대해 최대 block/results 2회(각2초) 조회를 수행하고 승인 L-D `Worker::reconcile(Command::Resolve)`에 원 증거를 전달한다. C writer gate 안에서는 영속 Attempt/TX 원문을 복사만 하고, gate 밖에서 조회한다. 이 복사는 방송 허가가 아니며 새 서명/방송을 하지 않는다.

조회 전후 동일 저장 snapshot·Context·freshness를 검사한다. Attempt의 first_possible_height..timeout_height 범위 밖·terminal Attempt·조회/증거/저장 오류는 lane을 닫고 재호출 IO를 거절한다. `None`은 그 한 블록의 미발견이며 저장·종결·정정을 하지 않는다. 포함 코드0/비0을 각각 INCLUDED_SUCCESS/INCLUDED_FAILURE 입력으로 옮기지만, C가 원 block/results/TX·history·hash·정수·불변 봉투를 다시 검증한 뒤에만 저장된다. 이 전이는 COMMITTED receipt나 잔고 확정이 아니다.

합성 순수시험은 fee0/25 × code0/1019의 실제 C store Resolve·원장/배치 보존·각각 두 번 replay, 미발견의 commit 불변, 위조 code 거절/오류 후 IO0, 조회 후 stale 저장0을 확인한다. 최초 시험은 정상적으로 갱신되는 attempt_refs/last_command_seq/stream_seq까지 불변으로 비교하여 실패했다. 비교 범위를 수정하고 실패 원문을 보존했다. 기존 fixture 공개 합성 키만 사용하며 RPC/listener/서비스0이다. `terminal-build.json`의 기존 offline/locked rlib와 Rust1.92.0 명령이 재현 근거다.

남은 실행 배선은 재시작 history 순회·absence/receipt/Apply·worker scheduling, 웹 ChainPort/mount, fee0/25 실제 초기화·launcher/cleanup/fault driver, 최종 binary/manifest·독립 pin 출처·CTO→Security다. 현재 runtime pin 미발급·DEV NOT_RUN이며 원 G00/ACK·부모 blocker를 유지한다.

## 저장된 관측의 Apply 배선

`SubmitLane::apply`는 최신 저장 snapshot·Context·freshness를 확인한 뒤 승인 L-D `Worker::reconcile(Command::Apply)`를 호출한다. RPC·새 서명·영수증 합성은 없으며 경제 전이는 C만 수행한다. 시각 오류·stale·anchor 불일치·C 거절 뒤 lane은 닫히고 재시도하지 않는다. 이 메서드는 receipt 존재 여부를 대신 판단하거나 timeout을 확정 실패로 바꾸지 않는다.

fee0/25의 미정산 PREPARED 배치가 있는 다음 높이를 적용하고 accounts/fills/batches·attempt·resolution/correction 보존과 두 번 replay를 검증한다. 이 시험은 잔고 확정 시연이 아니라 미정산 보류 보존 시험이다. stale·잘못된 anchor·시계 오류의 commit 불변도 확인한다. `apply-build-v2.json`과 `apply-tests-v2.log`가 exact 명령/결과이며 fixture는 공개 합성 데이터다. 최초 시험의 필드명 `balances`는 실제 schema의 `accounts`로 보정했다.

잔여: terminal raw TX/history 재시작 복구·receipt 저장·전체 worker scheduling, 웹 ChainPort·초기화/launcher·cleanup/fault driver·최종 binary/manifest·독립 pin 승인. 서비스/RPC0·DEV NOT_RUN이며 CTO→Security 제출 전이다.

## 승인된 trusted 복구 API 연결

C 복구 API head `77a5e68b5685b1eba4a8f5fc0c03915e74c39d77`, tree
`770d9883506d5edf34c4f6423fd2b37b0af4a51a`를 통합했다.
[CTO 승인](/NUS/issues/NUS-70#document-trusted-recovery-api-cto-review)
revision `00112aed-fde1-4cf1-ab87-0bd868363b0d`와
[Security 승인](/NUS/issues/NUS-70#document-trusted-recovery-api-security-review)
revision `c9298fb7-537d-4b24-b8a9-de6cc27166f7`의 동일 후보다.
이 component 승인은 최종 runtime pin이 아니다.

`recovery.rs::RecoveryCursor`는 C의 동일 View.commit에 history/attempt를 묶는다.
시작 시 observation 한 행과 applied/latest 두 anchor만 읽고, history는 호출당1..64행,
attempt는 순번 한 개씩 읽는다. 전체 history/attempt를 무제한으로 복사하지 않는다.
오류 또는 unwind 뒤 cursor는 닫히며 기존 부분 결과를 새 commit과 섞어 재시도하지 않는다.
새 commit의 복구는 전체 cursor를 버린 뒤 다시 시작해야 한다.
anchors/view는 고정 당시 자료이므로 효과 허가나 현재 freshness로 사용하면 안 된다.

`Observer::recover`는 마지막 저장 관측을 복원한다. 적용 C보다 앞선 관측도 유지하며,
복구 자체는 새 Observation·시각·방송·commit·REST 응답을 만들지 않는다.
다음 tick에서 신뢰 RPC를 읽고 기존 C freshness 및 effect gate를 통과해야 한다.
terminal TxRaw는 evidence 조회만 허용하고 기존 `with_committed_attempt` 거절을 보존한다.
이 모듈은 공개 route나 Serialize를 추가하지 않는다.

재현은 recovery-submit-build.json / recovery-observe-build.json의 rustc argv와
CARGO_MANIFEST_DIR, 현재 run scratch를 사용한다. 먼저 통합 checkout의 exchange에서
`cargo build --offline --locked --features dev-local-settlement --lib`를 실행한다.
Cargo.toml은 L-D 통합 설정이므로 C-only manifest와 구분한다.
서비스 기동·완성 worker/receipt/scheduling·웹 ChainPort·초기화/launcher·최종 manifest는
아직 완료되지 않았다. DEV NOT_RUN / runtime pin 미발급 / 원 G00·ACK 유지.

## 복구된 COMMITTED 영수증 저장 연결

`SubmitLane::committed_receipt`는 동일 C commit의 attempt 순번·terminal TX 원문과
해당 높이 history 1행을 읽는다. `SETTLE/INCLUDED_SUCCESS`만 받아 기존
`ChainRead::committed_receipt`로 조회하고 승인 C `Command::Receipt`에 원 증거를
전달한다. unresolved/실패/CLOSE attempt는 이 COMMITTED 경로에서 IO 전에 거절한다.
조회 전후 최신 snapshot·freshness, 조회 후 commit 동일성, Batch·terminal TX
동일성을 검사한다. 영수증 의미·wire·원 증거 검증은 C가 담당한다.
오류 뒤 lane은 닫히며 자동 재시도·새 서명·방송·Apply를 하지 않는다.

순수 fixture 시험은 fee0/25 각각 terminal 기록 후 재시작 → 영수증 저장 → 두 번
재생을 확인한다. 저장 단계에서는 accounts/fills/chain_snapshot/corrections가
변하지 않는다. 실제 확정 잔고의 Apply 시연은 포함하지 않는다.
위조 receipt wire, stale, RPC 오류와 미종결 attempt의 commit 불변도 검사한다.
명령과 결과는 receipt-store/build.json·compile.log·tests.log에 보존한다.

남은 범위: VOID/실패 증거·전체 worker scheduling, 웹 ChainPort, 초기화/launcher/
cleanup/fault driver, 최종 binary/manifest·독립 pin·CTO→Security. 서비스0·RPC0·
DEV NOT_RUN·runtime pin 미발급이며 원 G00/ACK와 부모 blocker는 그대로다.

## 확정 실패 저장 연결 / 다음 API 경계

`SubmitLane::reject_final`은 trusted snapshot·freshness를 대조하고 승인 C `Command::RejectFinal`에 위임한다. C가 저장된 시도·최신 관측으로 실패를 선택하고 검증한다. 오류 뒤 lane은 닫히며 CLOSE·VOID receipt·Apply를 자동 생성하지 않는다.

현재 C의 commit-pinned attempt/history 복구 API에는 별도로 저장된 ResolutionEvidence 원문/참조/closure가 없다. 정확한 VOID 연결을 위해 원 C 업무에 trusted read-only batch 실패 증거 복구 API를 요청한다. SRE가 원문을 재계산하거나 hash-only로 대체하지 않는다. 서비스0·runtime pin 미발급·DEV NOT_RUN이다.

## 승인된 실패 원문 복구 API 통합

C 후보 `8bbacf927358fb96bb028230de527b5dcc1dd6e4`를 통합했다.
[NUS-70 CTO 승인](/NUS/issues/NUS-70#document-failure-recovery-api-cto-review)
revision `c6b449be-3e4d-4133-a09c-f13791cf588a`와
[Security 승인](/NUS/issues/NUS-70#document-failure-recovery-api-security-review)
revision `777de971-fc0e-497e-91a6-1a340b3722b6`의 동일 후보다.
위 API 공백 기록은 이전 후보의 이력이며 이번 후보에서 해소됐다.

`RecoveryCursor::failure(batch_id)`는 cursor의 같은 commit으로 C의
`trusted_recovery_failure`를 호출한다. 저장된 typed 원문·참조·도달 Objects만
반환하며 SRE 재계산·자동 보충·방송 허가는 없다. 오류 후 cursor 전체가 닫힌다.
이전 home에 RejectFinal 원문이 없으면 새 C open은 실패하며 자동 수리하지 않는다.

fee0/25 실패 저장 뒤 원문/참조 대조와 두 번 replay, terminal callback 거절,
stale commit 거절 뒤 history/attempt/view 닫힘을 순수 fixture로 검증한다.
실제 VOID/CLOSE 전송·worker scheduling·웹 ChainPort·초기화/launcher·최종 manifest는
후속 작업이다. 서비스/RPC0·runtime pin 미발급·DEV NOT_RUN 유지.

이 통합의 build cache는 checkout 형제 `NUS-73-build-target`에 보존한다.
`CARGO_HOME=/Users/gangdongju/.cargo`, 설치 Rust 1.92.0의 절대 cargo/rustc,
`--offline --locked --features dev-local-settlement --lib`를 사용했다.
순수 submit 시험은 동일 dependency rlib들과 bech32를 명시적으로 연결한다.
최초 rustc 명령에서 bech32 누락으로 실패한 로그와 보정 argv를 증거에 포함한다.


## 승인 CLOSE API와 Account 연결

L-D 후보 `46546d317701b127da8196e9e6abdab7ce9a3d6e`를 fast-forward로 통합했다.
[NUS-71 CTO 검토](/NUS/issues/NUS-71#document-close-api-cto-review)
revision `b99ee8de-86dc-4a89-8695-ce69317aef9f`와
[Security 검토](/NUS/issues/NUS-71#document-close-api-security-review)
revision `d54094e3-73b7-47e3-87f9-56b030ce0bb7`에서 승인된 동일 후보다.

`SubmitLane::prepare_close`는 Account IO 전에 C commit을 고정하고, 동일 H/owner/키의
검증된 Account에서 number/sequence를 얻는다. 조회 후 freshness와 snapshot binding을
다시 확인하고 승인 `Worker::prepare_close`에 commit을 전달한다. C 저장 실패 원문과
closure 선택·서명·Attempt 전이는 L-D/C가 담당한다. SRE가 실패 원문을 재계산하지 않는다.
실패 또는 unwind 뒤 lane은 닫히며 후속 IO/서명을 거절한다. 이 메서드는 방송하지 않는다.

fee0/25의 실패 저장→재시작→CLOSE PREPARED/count0→두 번 replay, 원 실패 원문 보존과
자산 불변을 순수 fixture로 확인한다. 미해소 CLOSE의 재서명, Account 조회 오류·다른 높이·
조회 중 stale·동일 snapshot의 commit 경합은 서명 전에 거절되어야 한다. Account 원문은
비공개 audit 반환값이며 REST 응답이나 확정 영수증이 아니다. 공개 fixture 키는 시험에서만 쓴다.

잔여: VOID receipt 저장/worker scheduling, 웹 ChainPort, fee0/25 초기화·launcher·정리·
fault driver, 최종 binary/web manifest와 독립 runtime pin 및 CTO→Security 검토.
서비스/RPC0·DEV NOT_RUN·runtime pin 미발급·원 G00/ACK와 부모 blocker 유지.

### VOID 영수증 영속 연결

`SubmitLane::void_receipt`는 동일 C commit에서 성공 CLOSE TxRaw·terminal history·저장된 실패 원문 closure를 복구한다. `ChainRead::void_receipt` 결과의 VOID/batch/terminal TX/실패 참조를 대조하고 C `Command::Receipt`에 저장을 위임한다. 조회 뒤 commit 경합·stale·오류는 lane을 닫는다. 자산 해제/정정은 별도 승인 C Apply에 남는다. 순수시험은 fee0/25 재시작·두 번 replay 및 위조/조회 오류를 검증하며 실제 RPC/DEV 통합 결과가 아니다.

### timeout 관측 복구 연결

`RecoveryCursor::timeout_history`는 같은 commit의 미해소 Attempt에서 높이 범위를 얻어 정확히 8개 연속 저장 snapshot/원문을 반환한다. 종결 TX·timeout 전·없는 TX·commit 경합·불완전 범위는 cursor를 닫는다. 저장 관측은 freshness나 부재 증명이 아니다. `SubmitLane::resolve_absence`는 이 API로 history를 직접 복구하고 현재 snapshot과 대조한 뒤 Account·block/results·Batch 원 RPC 및 C의 기존 proof 검증에 위임한다. 호출자가 임의 history를 전달하는 실행 경로를 제거했다.

fee0/25 두 번 재시작에서 원문과 높이·commit/state 불변을 확인하는 신규 순수시험 3개를 추가했다. 실제 RPC/서비스 기동0, runtime pin 미발급, DEV NOT_RUN이다. worker scheduling·웹 ChainPort·초기화/launcher·정리/fault driver·최종 manifest/독립 승인 출처는 계속 남아 있다.

### bounded scheduler (기동 전 순수 검증)

`schedule.rs`와 `Observer::scheduled_tick`은 monotonic 주기·수명·총 tick
상한을 적용한다. tick마다 순차 관측을 먼저 저장하고 원 Observation을
작업 callback에 그대로 전달한다. catch-up 중 callback0이며 지연 뒤
몰아서 실행하지 않는다. 오류/panic/clock 역행 뒤 해당 scheduler는 닫힌다.
정산 상태별 action dispatcher와 process driver 연결은 아직 남아 있다.
서비스 실행·DEV 통합·runtime 승인을 의미하지 않는다.

## 재시작 후 과거 포함 높이 조회

`SubmitLane::resolve_historical_inclusion`은 현재 fresh Snapshot/Observation과 저장 TX hash, 조회할 높이를 받는다. C의 동일 commit에 고정한 trusted attempt/history 원문으로 해당 높이를 복원하고 신뢰 RPC block/results 최대2회(각2초)로 포함을 확인한다. 과거 snapshot에 새 Observation을 발급하지 않는다. 현재 관측 freshness·commit을 IO 전후 확인하며 C의 Resolve 검증에 최종 판정을 위임한다.

한 호출은 한 높이만 조회한다. 미발견은 저장 변경 없는 false이며 timeout 부재·VOID·정정 근거가 아니다. 높이는 attempt의 first..timeout 및 현재 높이 이하로 제한한다. terminal/범위 오류·IO 실패·stale·commit 경합 뒤 lane은 닫힌다. 기존 현재 높이 resolve도 동일 복구 경로를 사용한다. dispatcher는 후속 단계에서 미조회 높이를 순회해야 하며 현재 이 함수만으로 완성된 자동 복구 worker를 뜻하지 않는다.

## 포함 높이 순회 연결

`SubmitLane::scan_inclusion`은 C에서 복구한 unresolved Attempt의 정확한 8높이
범위를 한 호출당 한 높이씩 조회한다. 같은 hash의 성공한 미발견만 RAM cursor를
진행시키며 미래 높이는 IO 없이 기다린다. 재시작/hash 변경은 첫 가능 높이부터
다시 확인한다. 포함 결과는 기존 `resolve_at_with`의 원문·freshness·동일 commit
검증과 C Resolve를 통과해야 한다. IO/stale/terminal/오류/unwind 후 lane은 닫힌다.

`WindowScanned`는 scheduling 결과일 뿐 부재 증명이 아니다. 저장 상태/자산은
변하지 않으며 timeout 종결은 별도 전체 absence 원문 검증을 요구한다. cursor는
영속 증거로 저장하거나 REST에 노출하지 않는다. fee0/25 순수시험에서 한 tick 한
높이, 8높이 미발견 무변경, 두 번 재시작, 미래 높이 대기, 이후 포함과 종결 거절,
오류/panic 뒤 재호출0을 검증한다. 상태별 전체 dispatcher와 실행 파일 연결은
남아 있으며 실제 서비스0·DEV NOT_RUN·runtime pin 미발급이다.

## 미해소 Attempt 분기 연결

`SubmitLane::pending_tick`은 매 tick C의 영속 Attempt를 다시 읽어 한 action만
실행한다. PREPARED/count0·유효 operator/epoch·timeout 전에는 기존 영속 방송
API를 호출한다. UNKNOWN은 재서명/자동 재방송하지 않고 한 높이 포함 조회를
진행한다. 전체 범위 조회 후 timeout을 지난 경우에도 별도 전체 absence 원문을
다시 수집·검증한 뒤 C Resolve만 호출한다. 분기 결과는 영수증이 아니다.
오류와 unwind 뒤 lane을 닫으며 새 effect를 실행하지 않는다.

fee0/25 순수시험은 UNKNOWN 두 번 replay·한 action·오류/panic 재호출0과
scan 완료 뒤 전체 부재 proof 저장·자산 불변을 확인한다. 실제 RPC/서비스0,
DEV NOT_RUN·pin 미발급. 전체 batch dispatcher(Seal/terminal/receipt/Apply),
worker executable·웹 ChainPort·초기화/launcher·fault/정리·manifest는 남아 있다.

## 활성 batch dispatcher

`SubmitLane::active_tick`은 C의 동일 commit에서 미해소 batch 하나와 최대5개
Attempt 원문을 복구한다. 이미 영수증을 저장한 batch는 제외한다. 미해소
Attempt는 기존 pending dispatcher, 성공 SETTLE/CLOSE는 각각 원 COMMITTED/VOID
수집·저장, 종결 실패 SETTLE은 C RejectFinal, 저장된 실패 뒤에는 승인 CLOSE
준비로 연결한다. 전부 부재 입증된 SETTLE만 같은 batch로 다음 봉투를 준비한다.
상속 SETTLE3/CLOSE2 상한을 넘으면 오류로 닫히며 임의 실패/VOID로 바꾸지 않는다.

각 tick은 한 action만 실행한다. 준비와 방송, 실패 판정과 CLOSE 준비, 영수증
저장과 Apply를 같은 tick에 연쇄 실행하지 않는다. active batch가 없으면
`None`이며 이는 idle/Apply/Seal 중 어느 것도 승인하지 않는다. 바깥 driver의
Seal 목적 선택과 Apply 연결은 아직 남아 있다. 원문 recovery·C/L-D의 최종
검증이 권위이며 dispatcher 자체는 경제/정정 알고리즘을 구현하지 않는다.

선택 전 freshness, effect 전 동일 commit, 오류/panic 후 lane 닫힘을 적용한다.
시험은 fee0/25 실제 C store에서 최초 준비(count0), 두 번 replay 후 pending,
COMMITTED/VOID 원문 저장 뒤 자산 적용0·두 번 replay, 실패 판정 뒤 별도 CLOSE,
전체 부재 입증 후 동일 batch 재시도와 오류/panic 뒤 재호출0을 검증한다.
메모리 RPC·공개 합성 키를 사용하는 순수 component 시험이다.
실제 RPC/서비스0·DEV NOT_RUN·runtime pin 미발급. 전체 driver·실행 파일·웹
ChainPort·초기화/launcher·fault/정리·최종 manifest와 CTO→Security는 미완료다.

## 명시적 Seal 진입점

`SubmitLane::seal`은 trusted driver의 목적 문자열을 승인 `Worker::reconcile(Command::Seal)`에 전달한다. C가 FIFO·만료 여유·epoch·실패 목적을 검증하며 SRE가 재계산하거나 실패 시 다른 목적으로 재시도하지 않는다. browser 라우트 없음. stale/clock/C 거절 뒤 lane은 닫힌다. Seal은 attempt 생성·서명·방송·Apply를 수행하지 않는다. 목적 선택과 Apply/Seal 전체 dispatcher는 아직 후속 배선이다.

순수시험 `seal_lane_`는 fee0/25 정상 Seal의 accounts 불변·attempt0·동일 home 두 번 replay, 중복 Seal·잘못된 목적·근거 없는 RESOLVE_FAILURE·stale·clock 오류의 commit/state 보존과 lane 닫힘을 검증한다. 최종 결과와 rustc argv는 seal-lane artifact에 기록한다. 서비스/RPC0·runtime pin 미발급·DEV NOT_RUN.


## 관측 대사 바깥 dispatcher — 2026-10-07

`SubmitLane::reconcile_tick`은 저장된 같은 commit의 활성 batch를 먼저 처리하고, 활성 batch가 없고 최신 snapshot이 적용 anchor와 다를 때만 `Apply`를 위임한다. 두 anchor가 같으면 `Idle`이며 IO·Seal·서명·commit을 실행하지 않는다. 영수증 저장과 Apply는 별도 tick이다. stale/clock/기존 action 오류·panic 후 lane을 닫고 재호출하지 않는다. C의 경제·proof·정정·lock은 변경하지 않았다. 자동 Seal 목적 선택은 아직 별도 trusted driver 연결점이며 Idle을 Seal 승인으로 사용하지 않는다.

신규 순수시험3 PASS/0 FAIL: fee0/25 × COMMITTED/VOID 영수증 저장→재시작→C Apply→두 번 replay/Idle, 대기 fill의 자동 Seal0·commit 불변, stale/IO/panic 뒤 effect 재호출0. 시험은 메모리 RPC 원문과 공개 합성 키를 사용하며 실제 RPC·서비스0이다. 전체100개 중 기존97개는 이번 재실행하지 않았다. `reconcile-dispatch/build.json`, `compile.log`, `tests.log`에 실제 명령·결과를 보존한다. runtime pin 미발급·DEV NOT_RUN.

## 승인 C 준비 판단 연결 — 2026-10-07

위의 자동 Seal 미연결 기록을 이번 변경으로 갱신한다. C `20c0cd9`의
`trusted_reconcile_readiness`를 동일 commit·관측·시각으로 호출한다. 활성
batch 처리 후, Apply Ready이면 한 tick에 Apply만 실행한다. 다음 tick은 새
commit을 다시 조회한다. Apply가 보류되거나 관측이 없으면 C가 반환한 Ready
Seal 목적만 전달하고 Waiting이면 Idle이다. SRE는 FIFO·expiry·epoch 규칙을
복제하지 않으며 다른 목적으로 재시도하지 않는다. 오류/panic 뒤 lane 닫힘과
실제 C 실행 검증을 유지한다. 준비 판단과 분기 결과는 방송 권한이나 영수증이
아니다. 실제 worker 실행 파일·웹 ChainPort·초기화/launcher·fault/정리·최종
manifest는 후속 작업이며 서비스/RPC0·DEV NOT_RUN·runtime pin 미발급이다.
