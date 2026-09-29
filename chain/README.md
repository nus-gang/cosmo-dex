# S0-C Go 계약 구현

NUS-12. 기준선: S0-A rc2 `889fda0c7181a696b4eb2a2649508c6192af8406`.
공통 `protocol/`은 수정하지 않는다. `contract/schema.json`은 같은 schema의 고정 사본이며 시험에서 원본과 동일성을 검사한다.

## 재현

Go 1.24.4, darwin/arm64에서 실행했다. CI의 지원 플랫폼에서도 동일 명령을 사용한다.

```sh
bash chain/test.sh
cd chain
GOTOOLCHAIN=local go build -mod=readonly -trimpath ./cmd/contract-runner
go test -race ./...
go test ./contract -run '^$' -fuzz FuzzDecode -fuzztime=5s
```

`test.sh`는 Go JSON test event를 stdout으로 내보내고 실패 시 nonzero로 종료한다. 공통 CI는 저장소 루트에서 이 명령을 호출하면 된다. 결과를 캐시로 통과시키지 않도록 `-count=1`을 사용한다. SRE 소유 workflow는 수정하지 않았다.

CIRCL v1.6.3 / x/sys v0.28.0을 go.mod/go.sum에 고정했다. Go 1.24.4가 없으면 test.sh가 자동으로 다른 toolchain을 내려받지 않는다. 테스트 자산과 공개 fixture seed만 사용한다.

## 검증 범위

- strict protobuf wire: minimal varint, tag 순서, singular presence(0 포함), unknown/duplicate/wrong type 거절, 고정 폭, nested/repeated 제한과 decode/encode 동일성.
- API JSON: 중복/unknown key, 정수 JSON number/부호/지수/선행0 거절. U32/U64 및 decimal atoms↔16-byte U128, canonical hex/base64.
- 주문·취소·지갑 challenge의 pure ML-DSA-65, empty FIPS context. SHA256(raw pk)[:20] owner와 확정 등록 key type/raw bytes 비교. context/version/auth/epoch/expiry/market/fee-cap 순서의 스냅샷 검증.
- signatures.json 양성 3·음성 35. 양성은 fields→wire→frame→hash→실제 암호 검증과 공개 seed의 결정적 Go 서명 바이트까지 대조한다. 이 입력은 m0-crypto 프로필이며 DEV 시장 인수를 의미하지 않는다.
- wire-cases 8개 및 message-codec wire 10개, message-codec positives 14개, amount-codec 17개, integers.tsv 32개, policy-cases 13개를 소비한다. S0 cases에서는 Chain의 expiry/atoms/fill 15개를 소비한다.
- 등록 키/주소/도메인·context, 높이 999/1000/1001, 정수 최대값/overflow, JSON 부정 입력 및 resource limit의 추가 시험. JSONL 원시 로그에서 실제 개수·이름을 확인할 수 있다.
- race 및 짧은 parser fuzz를 실행했다. fuzz 실행률은 제품 TPS 지표가 아니다.

## 공통 runner

`contract-runner`는 stdin/stdout JSONL이다. `op=encode|decode|verify_crypto|verify`.

```json
{"op":"encode","message":"TransferStableV1","api_json":{"...":"message-codec.json의 api_json 전체"},"domain":"NUS/PAYMENT_ID/V1"}
```

encode/decode 결과: `code`, `wire_hex`, optional `sign_input_hex`/`sha256`, decode의 `api_json`.
verify_crypto 입력: `public_key_hex`, `sign_input_hex`, `signature_hex`. 순수 암호 결과만 반환한다.
verify 입력: `message`, `wire_hex`, `signature_hex`, `context` (`contract.Context`의 Go 필드명; RegisteredKey는 JSON base64). 이 runner의 context는 시험 harness가 주는 스냅샷이며 사용자 입력에서 확정 상태를 신뢰하면 안 된다. domain은 encode 진단용으로만 사용하며 Verify의 인증 도메인은 메시지 타입에 고정된다.

`Arithmetic`의 legacy TSV 진단(FORMAT/RANGE/OVERFLOW)은 상세 산술 시험용이다. API의 INTEGER_RANGE 변환은 상위 adapter 책임이다. 순수 Verify 성공은 원자적 nonce 소비·주문 ID 저장·잔고 예약을 수행하지 않는다.

## 미연결 경계

Cosmos SDK/CometBFT 앱, SIGN_MODE_DIRECT TX/ante handler, 계정 저장소·키 등록, Bech32 표시 adapter, 실제 genesis, x/stablecoin·x/exchange 상태 전이, ID 멱등 저장·누적 체결·보존식·출금/정산 경합은 **NOT_RUN / 미연결**이다. raw20 주소 결합만 이 패키지의 인증 경계다. `Context`는 호출자가 확정 상태에서 구성해야 한다.

Batch/receipt/state 사례의 영속성·정산 승인, 송금 권한/가스 후원, Wallet nonce 원자 소비는 이 순수 codec/인증 검증으로 증명하지 않는다. batches.json 전체 상태 판정과 message-codec의 state_cases는 이 runner에서 PASS로 집계하지 않았다. 실제 Rust/TS runner와의 생성·검증 전체 매트릭스는 Security S0-G, 새 checkout/전체 CI 인수는 QA S0-H가 수행한다. 현재 증거는 공유된 기존 다른 구현의 암호 fixture와 Go 일치다.

## 해시·출처

- contract aggregate: `daae05c8b3b94694d9f38122bf92a74f53b83197cd667d9ad93df545d55bc3dd`
- vectors aggregate: `60ee56ff3ff5a754965472cfb353dd1565072dbf0985907c369a0b78237f91e0`
- config: `7b12f8dffd4dfd07242331b975f0c440f5948074280c3a120c3232b3e674d13e`

파일별 무결성은 기준선 manifest와 대조한다. SDK v0.55.0·CometBFT v0.40.0은 기준선의 조사 대상이며 이 Go 모듈에 링크하지 않았다. SDK 실제 실행 성공으로 보고하지 않는다.

실제 사용 CIRCL의 [v1.6.3 ML-DSA 구현](https://github.com/cloudflare/circl/blob/v1.6.3/sign/mldsa/mldsa65/dilithium.go), [LICENSE](https://github.com/cloudflare/circl/blob/v1.6.3/LICENSE)와 x/sys v0.28.0 LICENSE를 내려받은 모듈에서 보존했다. 두 의존성은 BSD 3-Clause. Go 배포본 LICENSE도 evidence에 보존한다. 태그/합계는 go.sum 및 실행 manifest에 기록한다. 라이선스 법률 심사를 뜻하지 않는다.
