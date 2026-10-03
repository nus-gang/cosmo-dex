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

11개 시험: RPC 연속 높이/재시도/역행/블록 불일치/증거 저장 실패, canonical JSON/protobuf,
RPC URL, pipe 원문·동시성·UNKNOWN, 실제 HTTP CORS/헤더/상한, 실제 Rust 재시작·revision·
신선도. 시험 파일은 PAPERCLIP_RUN_SCRATCH_DIR가 있으면 그 안에 만들며 종료 때 정리한다.
S2_ENGINE_BINARY 미지정 시 실제 Rust 시험 하나가 skip이므로 전체 통과로 보고하지 않는다.
RPC 입력은 합성 fixture다. 실제 체인 예치 기반 S2-AT01/05/07 증거가 아니다.

## 실험 실행 인터페이스

실제 새 S2 genesis의 정확한 bytes와 해당 height 1 snapshot을 별도로 보존하고
Exchange [S2.md](../../exchange/S2.md)의 manifest를 만든다. 아직 자동 초기화 도구는 없다.
manifest의 genesis·owner·supply를 브라우저 입력에서 만들지 않는다. create 모드는
bootstrap height=1 및 실제 RPC 동일성을 요구한다.

```sh
python3 settlement/s2/server.py \
  --engine exchange/target/debug/exchange-s2 \
  --manifest .runtime/s2/manifest.json --genesis .runtime/s2/genesis.json \
  --journal .runtime/s2/engine-journal --evidence .runtime/s2/rpc-evidence \
  --bootstrap .runtime/s2/bootstrap.json --rpc http://127.0.0.1:26657 --port 8788
```

재시작에는 동일 파일·경로로 `--bootstrap`만 생략한다. 손상/부분 생성 journal을 삭제하거나
create로 대체하지 않는다. 기동 직후 CATCHING_UP, 신뢰 RPC 재관측 완료 후 OPEN이다.
새 S2 home과 journal을 사용하고 S1 경로를 재사용하지 않는다. HTTP의 `/s2/*`만 연결돼 있다.

## 남은 인수 작업

- S2 두 자산의 기존 DIRECT TX/receipt 경로 연결, 인증된 자기 조회와 공개 데이터 분리.
- 실제 S2 genesis 초기화·검증 도구, 실제 Chain RPC+HTTP+ML-DSA 주문/취소/부분 체결/IOC 시연.
- 실제 출금 epoch 변화의 양측 정정·재시작, D/P=0 정상 출금과 UNSETTLED_HOLD 인수.
- 재인증·계정 전환·지연 역순 응답·응답 유실 재시도 통합 시험과 Wallet/QA용 고정 실행 인계.
- 고정 head CI, CTO→Security 심사. main 통합은 CEO, 새 main checkout QA는 별도 담당.

P는 잠정 수취액이고 확정 C/가용액과 합치지 않는다. 체인 정산 제출은 비활성화다.
분산 내구성·처리량 목표·WS 전체 PASS를 주장하지 않는다.
