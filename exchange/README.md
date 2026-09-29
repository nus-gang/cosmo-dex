# S0-D Rust 계약 구현

`protocol/v1` rc2(`889fda0`)를 소비하는 독립 Cargo package. 합성 키·데이터만 사용한다. 이 제출은 서명·codec·산술·IOC 어댑터 계약이며 전체 거래소 runtime은 아니다.

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

`cargo test` 12개 시험 함수에서 위 벡터 집합을 소비한다. wire 순서 보존은 보장하지만 Batch의 경제적 정렬/영수증/원자 rollback 전체 검증은 이 라이브러리의 범위가 아니다. `batches.json` 경제 상태와 `message-codec.state_cases`는 Rust runtime PASS로 표기하지 않는다.

## SRE/G/H 인계

`cargo run --locked --bin vectors -- {vectors}` (`cwd=exchange`)는 `signatures.json` 절대 경로를 받고 stdout 단일 JSON을 출력한다. Cargo 로그는 stderr. pinned hash와 3/35 개수를 검사하여 누락·교체·실패 때 nonzero 종료한다. 출력 순서는 공통 파일 순서이며 필드는 `contract_revision`, `vectors_sha256`, `results[{id,sign_bytes_hex,valid}]`다. `contract_revision=1.0.0-rc2`; 이 값은 runner의 비교용 revision이고 `baseline_revision`이라는 M0 문서 UUID와 구별한다.

SRE manifest 입력 제안:

```json
{"contract_revision":"1.0.0-rc2","vectors":"protocol/v1/vectors/signatures.json","vectors_sha256":"4f55d2806f98ece56c4a7a7f148373ecaa82e6360c121310c124ed29db4f8217","rust_argv":["cargo","run","--locked","--bin","vectors","--","{vectors}"]}
```

CTO/SRE가 Go/TS 출력과 합의 후 공통 manifest에 연결한다. 이번 PR에서 타 담당자 `ops/ci/manifest.json`을 덮어쓰지 않는다. 교차 언어 실행은 G/H의 별도 인수 증거이며 Rust 단독 PASS로 대체하지 않는다.

## 해시·사용 소스

- contract aggregate: `daae05c8b3b94694d9f38122bf92a74f53b83197cd667d9ad93df545d55bc3dd`
- vector aggregate: `60ee56ff3ff5a754965472cfb353dd1565072dbf0985907c369a0b78237f91e0`
- config: `7b12f8dffd4dfd07242331b975f0c440f5948074280c3a120c3232b3e674d13e`
- 파일별 해시와 집합 계산 규칙: `protocol/v1/manifest.candidate.json`, `protocol/v1/README.md`.
- [OrderBook-rs v0.13.1](https://github.com/joaquinbejar/OrderBook-rs/tree/v0.13.1): peeled commit `a36218b9d2140e1c04ed22328f30fb4977adb109`, crate `.cargo_vcs_info.json` 일치, MIT. 실제 사용은 dev-dependency `=0.13.1`, default-features=false. 라이선스 고지는 `evidence/licenses/`에 보존. 태그·crate 메타데이터를 `evidence/`에 기록했다.
- [fips204 0.4.6](https://docs.rs/fips204/0.4.6/fips204/): MIT OR Apache-2.0, `ml-dsa-65`만 활성화. 런타임은 공개키 검증만 수행하며 개인키를 생성/저장하지 않는다.
- 전이 버전 및 checksum은 `Cargo.lock`. M0 실험과 전이 버전이 달라질 수 있어 이 lock을 사용한다. upstream fee/journal/sequence는 공통 계약의 금액·durability·명령 ID 근거로 재사용하지 않는다.

## 미연결 경계

WAL/outbox 원자 commit·durable ACK·프로세스 재생, 전역/다중시장 예약 장부, 사용자 요청 ID의 영속 바인딩·성공 재시도 영수증, 실제 계정 조회/키 타입 검증, wallet nonce 원자 소비, 취소/출금/정산 경합, 실체인 TX·직접 회수는 연결하지 않았다. `validate_order`는 **신규 주문의 순수 검증 함수**다. 영속 재시도는 wire/context/auth 검사 뒤 이전 영수증을 먼저 찾아야 하며 이 함수를 다시 호출해 성공 결과를 현재 만료로 덮어쓰면 안 된다. wallet policy는 합성 dev allowlist 전용 predicate이며 인증 endpoint가 아니다. 전체 예약/체인 연동·S0-G/H 독립 교차 검증·M1 이후 구현·실자산 운영 PASS를 주장하지 않는다.
