# S0-C Go 계약 구현

NUS-12. 기준선: S0-A rc4 `57c187f5474e02c3f624667d3b8380268e13dd1a`.
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

- contract aggregate: `afa3471a2210a90cde0004b3219b231468bf0b8471e8068f8e16b096b3549d84`
- vectors aggregate: `bb1b437d23a365f1083e85353bf62bcef6517cda94288ff7e2d0b4cb5fccd55b`
- config: `7b12f8dffd4dfd07242331b975f0c440f5948074280c3a120c3232b3e674d13e`

파일별 무결성은 기준선 manifest와 대조한다. SDK v0.55.0·CometBFT v0.40.0은 기준선의 조사 대상이며 이 Go 모듈에 링크하지 않았다. SDK 실제 실행 성공으로 보고하지 않는다.

실제 사용 CIRCL의 [v1.6.3 ML-DSA 구현](https://github.com/cloudflare/circl/blob/v1.6.3/sign/mldsa/mldsa65/dilithium.go), [LICENSE](https://github.com/cloudflare/circl/blob/v1.6.3/LICENSE)와 x/sys v0.28.0 LICENSE를 내려받은 모듈에서 보존했다. 두 의존성은 BSD 3-Clause. Go 배포본 LICENSE도 evidence에 보존한다. 태그/합계는 go.sum 및 실행 manifest에 기록한다. 라이선스 법률 심사를 뜻하지 않는다.

## rc3 수정 및 판정 포트

`op=fee`는 `receive`, `active_bps` 정규 십진 문자열을 받아 `value` 또는 오류 code를 반환한다. `op=cap`은 `cap`, `active_bps`를 받는다. receive U128을 먼저 검사하며 (0,0)은 0이다. 보존된 M0 integers.tsv I20의 이전 FEE_GE_RECEIVE 기대값만 rc3에 따라 명시적으로 0으로 대체한다. 원본 벡터는 수정하지 않았다.

`op=decide_order`는 `wire_hex`, `signature_hex`, `context`, `snapshot`을 받아 rc3 구조인 authentication/snapshot_policy/ack/wal_replay/ledger를 반환한다. 이 경로는 외부 authentication_result를 입력받지 않고 실제 ML-DSA를 실행한다. 등록 타입 누락은 NOT_CONNECTED이며 다른 타입은 ACCOUNT_KEY_MISMATCH다. 기존 verify는 호환용 인증+일부 정책 검사이며 완전한 주문 접수 결과가 아니다.

snapshot의 필드와 타입은 protocol/v1/DECISION-PORT.md와 같다. Context.SnapshotID와 Height, 서명된 q/p/cap/expiry, epoch 판정을 결합한다. rc4에서 불일치와 SnapshotID 누락은 NOT_CONNECTED/code=null이다. 원본 snapshot_id는 빈 문자열을 포함해 보존한다. snapshot에는 SYNTHETIC만 허용한다. 등록 키 타입/raw key·주소·서명은 인증 단계에서, ID/epoch/revoked/expiry/DEV 한도/active fee/cap/수취 수수료/누적 체결/확정 가용 잔고는 정책 단계에서 검사한다. 누적량·잔고는 합성 bool 전제이며 실제 원장 계산 증거가 아니다.

공통 fee 17/cap 21/decision 34개를 소비한다. decision 34개는 명시적 합성 인증 전제를 사용한 정책/구조 시험이다. 별도 14개 주문 사례는 공개 모의 seed로 변경 필드를 다시 서명하고 실제 ML-DSA를 실행한다. q=p=1, active=cap=25는 인증 OK/정책 FEE_GE_RECEIVE다. ACK와 ledger는 NOT_CONNECTED, WAL replay는 NOT_RUN으로 고정한다. Rust/TS 전체 differential, CI, Cosmos 앱·원자 상태·WAL/ACK는 이번 수정 검증 대상에서 미연결/NOT_RUN이며 기존 보안 FAIL·7차이를 해소했다고 선언하지 않는다.

## rc4 수정 검증

공통 snapshot-output 60건은 명시적으로 인증 결과를 주입해 production과 공유하는 내부 정책 경계를 검사한다. 실제 암호 60건으로 집계하지 않는다. 기존 실제 재서명 14건과 추가 binding 6건은 DecideOrder의 실제 ML-DSA 경로로 검증한다. 기존 rc3 원시 로그는 보존했다.

결합 검사를 정책 계산보다 먼저 수행하며 모순이면 NOT_CONNECTED/code=null로 반환한다. 인증 실패 시 NOT_RUN, ACK/ledger 미연결 및 WAL NOT_RUN은 유지한다. Go Context는 확정 상태 공급자가 구성하는 typed 입력이므로 Height/Epoch의 0도 명시적인 값이다. JSON adapter는 누락 상태를 0으로 채워 이 Context를 구성해서는 안 된다. 실제 앱/원자 상태/ACK/WAL, 전체 언어 differential 및 CI는 이 제출에서 NOT_RUN이다.

## CTO-RC4-01 JSON 입력 수정

`Context.UnmarshalJSON`은 Height/Epoch의 누락·null을 별도로 보존한다. DecideOrder는 인증 결과를 유지하고, 둘 중 하나가 누락/null이면 snapshot 정책을 NOT_CONNECTED/code=null로 반환한다. 명시적 숫자 0과 네이티브 Go Context의 0은 실제 값으로 검사한다. JSON decode 후 typed Context를 새로 조립해 presence를 잃어버려서는 안 된다.

`TestJSONLContextPresence`는 실제 runner 바이너리를 빌드하여 공개 공통 ML-DSA 주문 fixture를 stdin에 전달한다. Height=0 정책 PASS, Epoch=0 정책 EPOCH_MISMATCH 대조군 2건과 Height/Epoch 누락·null 4건을 검사한다. 6건 모두 인증 PASS, 원본 snapshot_id 보존, ACK/ledger NOT_CONNECTED, WAL NOT_RUN을 요구한다. 이는 합성 상태 입력 시험이며 실제 ACK/원장 증거가 아니다.

재현: `bash chain/test.sh` 또는 chain에서 `GOTOOLCHAIN=local go test -mod=readonly -count=1 -v ./cmd/contract-runner -run TestJSONLContextPresence`. 원시 결과: `chain/evidence/rc4-presence-tests.jsonl`. 기존 rc4 결과 및 보안 FAIL·7차이·783/782/1은 그대로 보존한다.
