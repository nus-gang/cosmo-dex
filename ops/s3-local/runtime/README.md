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
