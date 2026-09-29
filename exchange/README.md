# S0-D Rust 계약 구현

`protocol/v1` rc3(`549ce15`)를 소비하는 독립 Cargo package. 합성 키·데이터만 사용한다. 이 제출은 서명·codec·산술·IOC 어댑터 계약이며 전체 거래소 runtime은 아니다.

## 재현

저장소 루트, Rust 1.92.0, Python 3.9+:

```sh
python3 protocol/v1/tools/check.py
cargo fmt --manifest-path exchange/Cargo.toml --check
cargo test --manifest-path exchange/Cargo.toml --locked -- --nocapture
cargo clippy --manifest-path exchange/Cargo.toml --locked --all-targets -- -D warnings
cargo run --manifest-path exchange/Cargo.toml --locked --bin vectors -- protocol/v1/vectors/signatures.json
```

최초 의존성 다운로드 후 `--offline`도 가능하다. `.github/workflows/exchange-contract.yml`은 동일 Rust 시험을 실행한다. 성능 측정·처리량 보장은 하지 않는다. SRE scaffolding 브랜치의 같은 `exchange/` package 자리는 이 구현으로 대체해야 하며, 다른 컴포넌트/공통 protocol 파일은 변경하지 않았다.

## 구현과 검증

- `codec`: rc2 schema의 정규 API JSON↔protobuf wire. singular 0 presence, 순서/중복/unknown tag, minimal varint, U32/U64, bytes 고정 폭, ASCII, 반복 수/깊이/크기 제한, decode/reencode 동일성. API 정수는 십진 문자열, atoms=U128 16-byte BE, 주소/키/서명=canonical padded base64, hash=소문자 hex. `encode_json`은 중첩 JSON 중복 key도 거절한다. 이미 parse된 `Value`는 중복 key 정보를 잃으므로 네트워크 진입점에는 `encode_json`을 사용해야 한다.
- SHA256 raw key[:20], `nus` Bech32 lowercase roundtrip, frame·도메인. `fips204 0.4.6`의 실제 pure ML-DSA-65 verification, 빈 context만 허용. Rust 암호 라이브러리 검증 성공은 FIPS 인증 주장이 아니다.
- `validate_order`: wire→version→context→주소/확정 등록 키→signature→epoch/revoke/expiry→시장/fee→누계/확정 가용액 순서. `OrderContext`는 **확정 관측 상태로 호출자가 구성**한다. 클라이언트가 가용액·epoch·등록 키를 주입하게 연결하면 안 된다.
- fee의 U256 이하 중간값 및 U128 최종값, ceil, fee>=receive 거절. U64 lot/tick·누계 및 overflow. DEVBASE/DEVQUOTE 합성 min/max 1..1,000,000, lot당 1,000 atoms. 현재 S0 설정을 코드에 고정했으며 설정 변경은 해당 시험/코드 변경과 검토가 필요하다.
- IOC 예약 계약: 단일 시퀀서가 지정한 command_seq/match_index로 callback을 수집하고 원명령 완료 뒤 남은 R만 해제. 실제 upstream에서 `InsufficientLiquidity`가 반환돼도 이미 발생한 callback 체결은 반영한다. 가격 개선분도 D에 유지한다. 중복 callback/완료는 효과 1회, 같은 ID 다른 내용·순서 gap·초과 누계는 변경 없이 거절한다.

| 시험 집합 | Rust 검증 |
|---|---|
| signatures | 긍정 3 + 부정 35, fields→API→wire→frame→hash 독립 계산·실제 검증 |
| integers.tsv | 32, U128/U256/fee/underflow |
| amount-codec | 17, API↔wire 왕복과 잘못된 폭/문자열 |
| message-codec positives | 14, 송금·영수증 완전 API/wire 및 PAYMENT_ID hash |
| wire | 18, 공통 두 파일의 canonical/negative 사례 |
| policy | 13, 주문/취소 등호 및 wallet TTL/origin/audience/nonce 판정 |
| s0-cases Exchange 부분 | 15, expiry/atoms/fill; 나머지 receipt/submission/transfer 상태는 Settlement/Chain 소유 |
| 추가 | 주소·등록키·도메인·오류 우선순위·U32/U64/U128·누계·CLI receipt·IOC 순서/멱등 |
| 실제 upstream | 부분 체결 IOC Err+callback, 무체결 IOC, 취소 재시도 |

`cargo test` 16개 시험 함수에서 위 벡터 집합을 소비한다. wire 순서 보존은 보장하지만 Batch의 경제적 정렬/영수증/원자 rollback 전체 검증은 이 라이브러리의 범위가 아니다. `batches.json` 경제 상태와 `message-codec.state_cases`는 Rust runtime PASS로 표기하지 않는다.

## SRE/G/H 인계

`cargo run --locked --bin vectors -- {vectors}` (`cwd=exchange`)는 `signatures.json` 절대 경로를 받고 stdout 단일 JSON을 출력한다. Cargo 로그는 stderr. pinned hash와 3/35 개수를 검사하여 누락·교체·실패 때 nonzero 종료한다. 출력 순서는 공통 파일 순서이며 필드는 `contract_revision`, `vectors_sha256`, `results[{id,sign_bytes_hex,valid}]`다. `contract_revision=1.0.0-rc3`; 이 값은 runner의 비교용 revision이고 `baseline_revision`이라는 M0 문서 UUID와 구별한다.

SRE manifest 입력 제안:

```json
{"contract_revision":"1.0.0-rc3","vectors":"protocol/v1/vectors/signatures.json","vectors_sha256":"4f55d2806f98ece56c4a7a7f148373ecaa82e6360c121310c124ed29db4f8217","rust_argv":["cargo","run","--locked","--bin","vectors","--","{vectors}"]}
```

CTO/SRE가 Go/TS 출력과 합의 후 공통 manifest에 연결한다. 이번 PR에서 타 담당자 `ops/ci/manifest.json`을 덮어쓰지 않는다. 교차 언어 실행은 G/H의 별도 인수 증거이며 Rust 단독 PASS로 대체하지 않는다.

## 해시·사용 소스

- contract aggregate: `a71a8c03fea5e4d2876612e821eafcb4b359a0b132157929b9924d6f8fecd73e`
- vector aggregate: `4851d9d674b2412ca8919d8347a71da13f9adf4426fe60b44e2a4a259f8bd948`
- config: `7b12f8dffd4dfd07242331b975f0c440f5948074280c3a120c3232b3e674d13e`
- 파일별 해시와 집합 계산 규칙: `protocol/v1/manifest.candidate.json`, `protocol/v1/README.md`.
- [OrderBook-rs v0.13.1](https://github.com/joaquinbejar/OrderBook-rs/tree/v0.13.1): peeled commit `a36218b9d2140e1c04ed22328f30fb4977adb109`, crate `.cargo_vcs_info.json` 일치, MIT. 실제 사용은 dev-dependency `=0.13.1`, default-features=false. 라이선스 고지는 `evidence/licenses/`에 보존. 태그·crate 메타데이터를 `evidence/`에 기록했다.
- [fips204 0.4.6](https://docs.rs/fips204/0.4.6/fips204/): MIT OR Apache-2.0, `ml-dsa-65`만 활성화. 런타임은 공개키 검증만 수행하며 개인키를 생성/저장하지 않는다.
- 전이 버전 및 checksum은 `Cargo.lock`. M0 실험과 전이 버전이 달라질 수 있어 이 lock을 사용한다. upstream fee/journal/sequence는 공통 계약의 금액·durability·명령 ID 근거로 재사용하지 않는다.

## 미연결 경계

WAL/outbox 원자 commit·durable ACK·프로세스 재생, 전역/다중시장 예약 장부, 사용자 요청 ID의 영속 바인딩·성공 재시도 영수증, 실제 계정 조회, wallet nonce 원자 소비, 취소/출금/정산 경합, 실체인 TX·직접 회수는 연결하지 않았다. `validate_order`는 **신규 주문의 순수 검증 함수**다. 영속 재시도는 wire/context/auth 검사 뒤 이전 영수증을 먼저 찾아야 하며 이 함수를 다시 호출해 성공 결과를 현재 만료로 덮어쓰면 안 된다. wallet policy는 합성 dev allowlist 전용 predicate이며 인증 endpoint가 아니다. 전체 예약/체인 연동·S0-G/H 독립 교차 검증·M1 이후 구현·실자산 운영 PASS를 주장하지 않는다.


## rc3 G-01~04 수정 인계

Security→QA가 승인한 공통 SHA `549ce150d6a9f21ec30f159d39a4d91c31dbd759`를 병합했다. protocol 원본은 변경하지 않았다. 기존 rc2 증거 `evidence/manifest.json`·`test.log`는 역사 기록이며 이번 결과는 `evidence/rc3-*`다. 기존 보안 FAIL·7차이는 소급 수정하지 않는다. 수정 후 Go/Rust/TS 독립 교차검증은 NOT_RUN.

- G-01: `OrderContext.registered_key_type`과 raw key를 최상위 `validate_order` 및 `authenticate_order`에서 검사한다. 미등록은 ACCOUNT_KEY_UNREGISTERED, OTHER/다른 bytes는 ACCOUNT_KEY_MISMATCH, 타입 미연결은 NOT_CONNECTED. 정상 타입도 owner 바인딩·실제 ML-DSA 검증이 필요하다.
- G-02: U128 입력 → active bps 0..10000 → 0 bps 반환 → 양수 ceil/fee>=receive 거절. 역사 `integers.tsv:I20`은 (0,0)을 거절하지만 CONTRACT rc3 우선 적용 조항과 `decision-port:fee-0`에 따라 0이다. 시험에서 이 하나의 규범 변경을 명시적으로 대조하며 원본 파일을 바꾸지 않았다.
- G-03: 서명 cap은 U32 전체 범위, active는 별도 0..10000 및 active<=cap. cap 10001/U32_MAX도 새로 서명해 실제 검증했고 cap으로 active 범위를 완화하지 않는다.
- G-04: `decision::admit_order`는 실제 인증, 명시적 SYNTHETIC snapshot 정책, ACK를 분리한다. q/p/cap/expiry 및 snapshot id/height가 인증된 주문·관측값과 다르면 NOT_CONNECTED. 필수값 누락·null·잘못된 bool은 기본값으로 채우지 않는다. ack=NOT_CONNECTED, ledger=NOT_CONNECTED, wal_replay=NOT_RUN 고정이다. snapshot 입력은 신뢰된 시험 harness 전용이며 서버의 확정 상태 조회와 연결하지 않았다.
- `evaluate_snapshot`은 합성 인증 결과를 주입하는 정책 전용 함수다. 공통 decision 34개는 이 함수의 정책·출력 구조 증거이며 실제 암호 증거는 별도 등록키·변조·재서명 시험이다.
- `validate_order`는 기존 신규 주문 convenience 함수이고 ID 영속 판정·재시도나 ACK를 구현하지 않는다. G/H는 분리된 `admit_order` 출력을 소비한다.

JSON-lines 재시험 포트: `cargo run --manifest-path exchange/Cargo.toml --locked --bin decision`. stdin 1행 JSON당 stdout 1행 JSON. `op=fee`는 receive/active_bps 정규 십진 문자열, `op=cap`은 cap/active_bps를 받는다. 성공 result 문자열 또는 error code를 반환한다. 실제 인증은 `op=admit_order`, wire_hex, signature_hex, context, snapshot을 받는다. context의 필수 필드는 snapshot_id/height/epoch/chain_id/genesis_hash/exchange_module_id/market_id/market_config_version/registered다. registered=null은 미등록, 객체는 key_type/raw_key_hex를 명시한다. snapshot 필드는 공통 DECISION-PORT.md와 같고 숫자는 문자열이다. context/등록 조회 미연결은 승인으로 바꾸지 않는다. 입력 예제는 `tests/rc3.rs::json_lines_port_uses_real_registration`에 있다.

재현 환경: Rust 1.92.0, macOS arm64, Cargo.lock 고정. 모의 seed로 생성한 테스트 키만 사용한다. 명령은 위 재현 절차와 동일하며 rc3 단독 재시험은 `cargo test --manifest-path exchange/Cargo.toml --locked --test rc3 -- --nocapture`. CI의 기존 all-targets 시험에 자동 포함된다. SDK 탐색 환경 경고가 있었으나 컴파일/16 tests/Clippy 모두 성공했다.


## G-RC3-01/02 admission 수정

`admit_order`는 서명 인증 후 `validate_order`와 같은 시장 규칙(side/order_type 1/2,
q/p 1..1,000,000, RECEIVE_ASSET_V1)을 정책 단계에서 검사한다. 인증 성공만으로 정책 PASS를 반환하지 않는다.
서명 owner_epoch와 신뢰 context.epoch의 비교 결과가 snapshot.epoch_matches와 같아야 한다.
일치/true는 정상, 불일치/false는 EPOCH_MISMATCH, 두 모순은 NOT_CONNECTED다.
기존 공통 오류를 사용하며 새 공통 표준 오류를 추가하지 않았다. CLI의 context.epoch는 정규 U64 십진 문자열 필수값이며 누락/비정규 값은 NOT_CONNECTED다.

재현:

```sh
NUS_ADMISSION_EVIDENCE="$PWD/exchange/evidence/admission" cargo test --manifest-path exchange/Cargo.toml --locked -- --nocapture
cargo clippy --manifest-path exchange/Cargo.toml --locked --all-targets -- -D warnings
python3 protocol/v1/tools/check.py
```

17 tests PASS: 새 시험은 모의 ML-DSA 키로 enum 9조합, 시장 7경계, epoch 4조합을 새로 서명하고 함수와 실제 CLI에서 검사한다.
CLI epoch 누락 1건을 더해 21요청이며 evidence/admission/inputs.json과 rust-results.json에 원시 입력·결과를 저장한다.
Go/TS 제한 비교는 `tools/compare_admission.py <cross-root>`로 실행한다. cross-root에 아래 고정 SHA의 경로를 git archive로 추출하고,
chain에서 `go build -mod=readonly -o ../security/go-runner ../security/go.go`, web에서 `npm ci --ignore-scripts`를 실행한다.
이번 로컬 TS 의존성은 같은 lock의 기존 설치를 읽기 전용 재사용했으며 fresh install 증거는 아니다.

- Go chain: 5d39b7a0ffe911ed60cd0e3a56ada135e76a425f
- TS web: 8a70632d933b58c15b5bbac9fbef25dfa7312643
- protocol: 549ce150d6a9f21ec30f159d39a4d91c31dbd759
- security/go.go 및 security/runner.ts: 068477c (전체 SHA는 admission/manifest.json)

같은 wire/signature/context/snapshot을 Go/Rust/TS에 전달한 60비교 중 54일치, 6차이:
Rust 20/20, Go 20/20, 기존 TS 14/20. TS의 side/order_type에 3이 포함된 5조합과 fee_asset_policy_id=OTHER에서 정책 PASS가 남는다.
Wallet 원본은 수정하지 않았다. 이전 SHA를 고정한 구현자 회귀 비교이며 Wallet 최신 수정 검증 또는 Security 독립 전체 재시험이 아니다.
모순 snapshot의 세부 code/id는 표준화되지 않아 REJECTED/NOT_CONNECTED 불변식으로 비교한다.
기존 Security 759/754/5 FAIL·암호 상호운용 PASS와 이전 420/7 기록을 보존한다. 전체 독립 수정 후 재시험 NOT_RUN.
ACK·원장·REST/WS/체인 NOT_CONNECTED, WAL/replay·직접 회수 NOT_RUN. main merge·출시·후속 기능 승인 없음.
