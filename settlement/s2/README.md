# S2-D REST/RPC 연결 — 진행 중

[NUS-39](/NUS/issues/NUS-39)의 구성요소 체크포인트다. 전체 API 인수·전문 심사 완료가 아니다.
승인된 Exchange 프로세스의 서명·세션·예약·journal을 그대로 사용한다. Python 표준
라이브러리만 추가하며 공통 protocol, Rust 구현, S1 파일을 변경하지 않는다.

## 고정 기준

- 원격 main: `24029b811e5ec798bbe57f769de3d3f254c90ab7` (2026-10-03 조회)
- 계약 [NUS-36](/NUS/issues/NUS-36): `3571c3a331a169a8eef231aad14f7816908dd2c5`
- Chain [NUS-37](/NUS/issues/NUS-37): `7ce952475b4985096344d713467208722b5e7693`, PR #28
- Exchange [NUS-38](/NUS/issues/NUS-38): `d0cf18f1294941b19e72102568eb220a7a5854f1`, PR #27
- contract hash: `2e103517c344f21c2b97fbe7e977f0e614c32c704b0f54978c5bb1fb3ae6ab0f`
- config hash: `70281595d471947a56d9bf8a97553dd388a85b107c215a95cc6f34ea9f5f321f`

## 구현된 경계

`transport.py`는 한 Rust child의 stdin/stdout을 잠금으로 직렬화한다. HTTP body는 원문
bytes 그대로 request envelope에만 넣는다. 브라우저 JSON의 `op=observe`를 제어 명령으로
승격하지 않는다. 응답 EOF/timeout/잘못된 프레임은 child를 종료하고 이후 요청도
UNKNOWN으로 닫는다. 지연 응답이 다른 계정의 요청 응답으로 잘못 전달되지 않는다.
새 journal 생성·복구·재시도는 자동 수행하지 않는다.

`chain.py`는 literal loopback RPC만 사용하며 환경 proxy와 redirect를 사용하지 않는다.
각 H의 ABCI response height, snapshot hash/context/시장/owner/supply, 같은 H의 block
height/hash/chain/time을 대조한다. 원시 ABCI와 block 응답은 내용 hash 기반 파일에
fsync하고 directory fsync한 뒤 엔진에 보낸다. 경제 불변식·등록키는 Rust가 추가 검증한다.
이것은 신뢰 RPC 동일성 검증이며 light-client proof가 아니다.

cursor는 별도 Python DB에 복제하지 않고 엔진의 복구된 observed_height에서 시작한다.
동일 H를 재관측한 뒤 H+1을 순차 소비하며 빈 블록도 생략하지 않는다. 한 pass는 최대
8개 높이로 제한한다. backlog가 있으면 먼저 접수를 닫고 마지막 높이 전까지 catching_up을
유지한다. 실패 시 마지막 높이를 유지하고 rpc_failed를 전달한다. 높이 역행은 fail-closed로
고정하며 검증 가능한 낮은 H가 있으면 엔진의 HEIGHT_REGRESSION 상태도 설정한다.

`server.py`는 `127.0.0.1`에만 bind하고 정확한 두 Origin, Host, body 16384B 상한,
중복 헤더 거절, chunked 거절, 16개 연결 상한, no-store/Vary를 적용한다. 개인 GET에도
Origin과 bearer를 엔진으로 전달한다. 경제적 상태·HTTP code·revision은 엔진 출력 그대로다.
SIGTERM/키보드 종료 때 collector와 엔진을 종료한다. Python은 비밀키를 받거나 서명하지 않는다.

## 시험

저장소 루트에서 기존 Rust 1.92.0과 Cargo.lock을 사용한다.

```sh
cargo build --locked --offline --manifest-path exchange/Cargo.toml --bin exchange-s2
S2_ENGINE_BINARY="$PWD/exchange/target/debug/exchange-s2" \
  python3 -m unittest discover -s settlement/s2 -v
```

21개 시험(기존 14개와 DIRECT/receipt 경계 7개): RPC 연속 높이/재시도/역행/블록 불일치/증거 저장 실패, canonical JSON/protobuf,
RPC URL, pipe 원문·동시성·UNKNOWN, 실제 HTTP CORS/헤더/상한, 실제 Rust 재시작·revision·
신선도. 시험 파일은 PAPERCLIP_RUN_SCRATCH_DIR가 있으면 그 안에 만들며 종료 때 정리한다.
S2_ENGINE_BINARY 미지정 시 실제 Rust 시험 하나가 skip이므로 전체 통과로 보고하지 않는다.
RPC 입력은 합성 fixture다. 실제 체인 예치 기반 S2-AT01/05/07 증거가 아니다.

## 실험 실행 인터페이스

실제 새 S2 genesis의 정확한 bytes와 해당 height 1 snapshot을 별도로 보존하고
Exchange [S2.md](../../exchange/S2.md)의 manifest를 만든다. `bootstrap.py`는 실제 genesis bytes와 승인 profile로 manifest를 고정하고, RPC height 1/header 및 등록키를 검증한다. 출력 디렉터리가 이미 있으면 거절하며 실패한 증거를 보존한다.
manifest의 genesis·owner·supply를 브라우저 입력에서 만들지 않는다. create 모드는
bootstrap height=1 및 실제 RPC 동일성을 요구한다.

```sh
python3 settlement/s2/bootstrap.py \
  --genesis .runtime/s2/node/config/genesis.json \
  --output .runtime/s2/bootstrap --rpc http://127.0.0.1:26657

python3 settlement/s2/server.py \
  --engine exchange/target/debug/exchange-s2 \
  --manifest .runtime/s2/bootstrap/manifest.json --genesis .runtime/s2/bootstrap/genesis.json \
  --journal .runtime/s2/engine-journal --evidence .runtime/s2/rpc-evidence \
  --bootstrap .runtime/s2/bootstrap/bootstrap.json --rpc http://127.0.0.1:26657 --port 8788
```

재시작에는 동일 파일·경로로 `--bootstrap`만 생략한다. 손상/부분 생성 journal을 삭제하거나
create로 대체하지 않는다. 기동 직후 CATCHING_UP, 신뢰 RPC 재관측 완료 후 OPEN이다.
새 S2 home과 journal을 사용하고 S1 경로를 재사용하지 않는다. HTTP의 `/s2/*`와 `/s1/txs`, `/s1/txs/{TX_HASH}`, `/s1/accounts/{bech32_owner}/requests/{request_id}`가 연결돼 있다.

## 남은 인수 작업

- 실제 체인 단절·높이 역행 통합 검증과 4검증인 후보 인계 (양측 정정·정상 출금/보류 검증 완료).
- 재인증·계정 전환·지연 역순 응답·응답 유실 재시도 통합 시험과 Wallet/QA용 고정 실행 인계.
- 고정 head CI, CTO→Security 심사. main 통합은 CEO, 새 main checkout QA는 별도 담당.

P는 잠정 수취액이고 확정 C/가용액과 합치지 않는다. 체인 정산 제출은 비활성화다.
분산 내구성·처리량 목표·WS 전체 PASS를 주장하지 않는다.

## 실제 RPC 체크포인트 (2026-10-03)

`check_live.py --chain <승인 nusd> --engine <exchange-s2> --operators <operator-accounts.json> --output <새 디렉터리>`는 새 S2 genesis/단일 검증인에서 실제 DEVBASE·DEVQUOTE 예치, 직접 DEVBASE 출금, epoch 변화 관측, 엔진 재시작을 검증한다. `evidence/live-bootstrap/result.json`과 원시 snapshot/RPC를 보존했다. 실제 서명 HTTP 주문·양측 정정은 후속 체크포인트에서 검증했고 4검증인 통합은 NOT_RUN이다. 테스트 키만 사용하며 종료 시 체인과 엔진을 종료한다.

## DIRECT HTTP 체크포인트

`direct.py`는 `/s1/txs`에 전달된 서명 TxRaw를 변경 없이 별도 genesis-bound evidence에
fsync한 뒤 trusted S2 RPC로 제출한다. 개인 주문 인증이나 엔진 OPEN을 직접 출금의
권한으로 사용하지 않는다. 서버 전체가 정지하면 기존 체인 RPC/CLI 직접 TX 경로를 사용한다.
CheckTx 성공도 202 `SUBMISSION_UNKNOWN`이며, `/s1/txs/{TX_HASH}`는 조회 tip 이하의
실제 block 포함과 block_results/index 결과 일치 후에만 `COMMITTED` 또는
`REJECTED_FINAL`을 반환한다. 조회 실패는 UNKNOWN이다. 시작 시 자동 재전송하지 않는다.

DIRECT HTTP JSON은 base64 팽창을 위해 22000B, 실제 TxRaw는 기존 16384B 상한이다.
S2 주문 JSON 상한 16384B는 유지한다. POST에는 기존 Origin/Host 검사를 적용하며
TX hash별 공개 체인 결과 조회만 무인증으로 허용한다. owner/request receipt는 아래의 체인 공개 조회 경로를 사용한다.

CI source manifest는 `python3 ops/ci/build_manifest.py`로 생성한다. settlement/s2도
기존 S0 manifest의 소스 추적 대상이므로 해당 파일 변경 뒤 재생성하고 `--check`를 실행한다.
검사 제외 경로나 oracle 기대값을 바꾸지 않는다.

## 실제 서명 HTTP·양측 정정 체크포인트

`test_signer.go`는 nusd의 공개 개발용 seed 2개만 사용한다. 기존 `chain/go.mod`의
codec/ML-DSA를 재사용하며 HTTP 서버에서 이 도구를 호출하지 않는다. 임의 키 입력은 없다.

```sh
(cd chain && go build -o "$PAPERCLIP_RUN_SCRATCH_DIR/s2-test-signer" ../settlement/s2/test_signer.go)
python3 settlement/s2/check_live.py \
  --chain ../NUS-37/chain/app/bin/nusd --engine exchange/target/debug/exchange-s2 \
  --operators ../NUS-37/chain/app/config/operator-accounts.json \
  --signer "$PAPERCLIP_RUN_SCRATCH_DIR/s2-test-signer" \
  --output "$PAPERCLIP_RUN_SCRATCH_DIR/live-orders" --port 29957
```

새 genesis에서 실제 두 자산 예치→HTTP WalletChallenge/ML-DSA 주문→부분 체결→
서명 잔량 취소→빈 주문장 제한 IOC→UNSETTLED_HOLD→DIRECT 출금 epoch 양측 정정→
엔진 재시작·재인증→D/P=0 준비 OK 및 실제 HTTP 출금을 확인했다. PENDING/CORRECTED
fill ID와 양측 C/R/D/P/A, 원본 요청·receipt, RPC snapshot·block 응답을 보존한다.
동일 요청 재시도는 원래 receipt, 다른 계정 조회는 404, nonce 재사용/옛 세션은 401이다.

이는 단일 검증인 실제 체인 시험이다. 첫 응답을 버리고 재시도했으며 네트워크 중간
응답 유실 장애 주입은 아니다. IOC는 빈 주문장 무체결 경계이고 가격 한도 내 체결
시험을 대신하지 않는다. 4검증인·브라우저·전체 S2 인수·main QA PASS가 아니다.


## Receipt·응답 유실·부분 체결 IOC 추가 검증

S1 `/s1/accounts/{bech32_owner}/requests/{64 lowercase hex request_id}`는 공개 체인
receipt다. S2 snapshot의 base64 20-byte owner를 `nus` bech32로 변환해 등록 계정
allowlist를 확인한다. S2 `/s2/me/commands/{kind}/{request_id}?epoch={epoch}`는 별도로
WalletChallenge 세션이 필요한 개인 주문 receipt다. 두 owner 표현을 혼용하지 않는다.
체인 receipt는 검증한 snapshot 높이를 ABCI 조회에 고정하고 chain/genesis/owner/request ID/
committed height를 대조한다. 응답 height가 다르거나 RPC가 끊기면 503 UNKNOWN이다.
404 NOT_FOUND_AT_HEIGHT는 그 관측 높이에서 없다는 뜻이며 제출 실패 확정이 아니다.

실제 개발망 시험은 HTTP handler가 엔진 응답을 받은 뒤 status/header/body를 전송하기 전에
연결을 닫는다. 클라이언트 RemoteDisconnected 이후 개인 receipt 조회 및 동일 원문 재시도로
효과 1회를 검증한다. 이 장애 주입은 시험 전용 handler이며 제품 서버에는 활성화 경로가 없다.
별도 maker 500 lots, IOC 1000 lots/limit 10001 ticks는 10000 ticks에서 500 lots를 체결하고
잔량을 취소한다. 매수 D는 기존 10000000 + 신규 5000500 = 15000500 atoms를 유지한다.
두 fill ID는 실제 직접 출금 epoch 변화 후 양측 CORRECTED, 엔진 재시작 후에도 동일하다.
검증인 실제 중단/재기동 시 마지막 높이 보존·접수 닫힘·receipt UNKNOWN·회복도 시험한다.

4검증인 통합·브라우저·실제 main 인수는 별도 담당 인수 대상이며 아직 PASS로 표시하지 않는다.
