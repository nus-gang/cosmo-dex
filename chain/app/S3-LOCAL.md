# S3 로컬 개발 Chain 바인딩

[NUS-55](/NUS/issues/NUS-55)의 비활성 호환성 후속이다. 승인 입력은
[NUS-54](/NUS/issues/NUS-54)의 `fd9aa6ca9093817e4ab09d2ae835197a84bbade6`,
후보 manifest `90169d322336a0c0de9bc6c48725d528d42fe74c78ea5b596fc7e059d747dda2`다.
기존 B `f44d511bee7ced4f95dce029ddd7abcf7d8e6722`와 표준 `s3/1` 경로를 보존한다.

`G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`. 실제 DEV01~14는 NOT_RUN이다.
이 문서의 검증은 SDK component fixture이며 최종 runtime manifest·4검증인·engine·worker·REST·브라우저 통합 승인이 아니다.

## 호출 경계

개발 어댑터는 Go build tag `dev_local_demo`를 명시해야 컴파일된다. 기본 빌드에서는
`s3_local_demo.go`가 제외되고 `NewForChain`/`DecodeS3Genesis`는 이전 고정 hash를 검사한다.
기존 `nusd`에는 개발 옵션이나 자동 fallback을 추가하지 않았다.

향후 별도 `nus-s3-local-demo` launcher의 `dev-local-demo` feature에서
`--local-demo-profile`과 `--acknowledge-unproven-space`를 모두 확인한 뒤에만
다음 API를 사용한다. launcher·binary·서비스 기동 구현은 이번 패치에 포함하지 않는다.

```go
// go build -tags dev_local_demo
context, err := app.ValidateLocalDemo(inputs) // 원문 검증만 수행
chain, err := app.NewLocalDemo(db, logger, inputs)
```

`LocalDemoInputs`에는 독립 심사 인계에서 받은 `ApprovedRuntimeSHA256`, runtime manifest 원문,
manifest에 열거된 모든 파일 원문, 선택 effective profile 원문, canonical guard 원문,
exact genesis 원문, 명시적 `AcknowledgeUnprovenSpace=true`를 전달한다.
profile 원문이나 확인 값이 없으면 거절한다. manifest에서 읽은 hash를 스스로 승인 pin으로
사용하면 심사 권위가 사라진다. API는 byte 일치만 검증하며 사람의 승인 사실을 증명하지 않는다.

호출자는 검토된 launcher/store를 통해 새 root의 0700·canonical path/inode·symlink/hardlink 거절·
writer lock·불변 guard의 no-replace/fsync를 먼저 충족해야 한다. Chain API는 원문을 소비하며
파일을 열거나 guard를 발급하지 않는다. 이 경계의 실제 파일 생명주기, S3D1/S3W1, durable ACK,
loopback 서비스, ENOSPC 복구는 이번 SDK 시험의 PASS에 포함하지 않는다.

## manifest 입력 형식

아래 JSON은 **형식 설명**이다. 실행 가능한 최종 manifest가 아니다.

```json
{
  "format": "s3-dev-local-runtime/1",
  "scope": "REVIEWED_RUNTIME",
  "candidate_manifest_sha256": "<승인 후보 MANIFEST.json SHA256>",
  "contract_sha256": "<아래 파일 집합의 집계 SHA256>",
  "files_sha256": {"<repo-relative path>": "<exact bytes SHA256>"},
  "components": {
    "chain": "chain/local-demo/components/chain.json",
    "exchange": "chain/local-demo/components/exchange.json",
    "settlement": "chain/local-demo/components/settlement.json",
    "wallet": "chain/local-demo/components/wallet.json",
    "sre": "chain/local-demo/components/sre.json"
  }
}
```

component descriptor는 `head`, `tree`(lowercase git SHA40), `implementation_settings`(문자열 map)의
세 필드다. 검토된 component pin과 선언 설정을 해당 원문에 넣는다. descriptor도 집계 대상이다.
이 형식은 Chain 어댑터 입력 codec이며 공통 `protocol/` 또는 승인 계약 파일을 바꾸지 않는다.
CTO가 새 심사에서 호출 계약을 검토한다.

파일 집합은 승인 rc3 manifest의 204개 원본, `protocol/s3/manifest.json`, 승인 후보의 8개 파일,
위 5개 descriptor로 정확히 구성한다. 런타임 manifest 자체·genesis·키·실행 결과나 임의 파일은
허용하지 않는다. 집계는 `SHA256(sorted sha256 + two spaces + repo-relative path + LF)`다.
후보 manifest와 rc3 manifest 자체의 SHA를 코드에서 pin하고 그 안의 모든 파일 byte를 다시 검증한다.

`Files`는 봉인된 승인 소스 snapshot이다. 현재 작업 디렉터리를 통째로 읽어 다시 봉인하지 않는다.
기존 B가 승인받은 `chain/app/go.mod` 로컬 codec require/replace 때문에 rc3 원본 lock과
구현 lock은 다르다. rc3 원본은 그대로 제공하고 실제 구현 lock SHA는 component 설정에 기록한다.
fixture의 `testdata/local-demo/rc3-chain-app-go.mod`는 `fd9aa6c`의 exact 원본이다.
외부 의존성·현재 구현 go.mod/go.sum 변경은 없다.

`COMPONENT_FIXTURE`와 Chain descriptor 하나를 쓰는 경로는 내부 `_test.go` helper만 호출한다.
공개 API는 이를 거절하고 다섯 component를 요구한다. 전체 형식의 단위시험도 합성 descriptor와
명시적 시험 pin을 사용한다. 그 결과를 실제 component 심사 또는 최종 runtime 승인으로 간주하지 않는다.

## 초기화·조회·재시작

- effective profile bytes는 승인 후보의 fee0 또는 fee25 파일과 정확히 같아야 한다.
  guard의 8개 필드와 Context 전체를 검사한다. 누락·추가·중복·대소문자 변경·비정규 JSON을 거절한다.
- Context는 `s3/3`, `nus-s3-dev-1`, exact genesis SHA, 집계 contract SHA,
  exact effective profile SHA, `DEVBASE/DEVQUOTE`, market config `1`이다.
- genesis app_state의 contract/config/fee도 Context와 일치해야 한다. `InitChain` 요청의 시간,
  chain ID, initial height, app_state 원문, consensus 값, validator key/power 집합을 genesis와 대조한다.
  Comet의 validator 정렬은 허용하되 키/권한 집합 변화는 거절한다.
- SDK cache를 초기화하기 전에 잘못된 InitChain을 거절한다. 올바른 요청 재시도가 가능하다.
- genesis namespace의 exchange 원장과 별도로 불변 `s3_binding`에 guard 원문·contract/config/fee를 저장한다.
  다시 열 때 genesis/chain과 저장 바인딩 전체 및 fee/version이 같아야 한다. 기존 DB에 guard만 바꾸거나
  표준↔개발 DB를 교차 개방할 수 없다.
- Snapshot/Batch/Order 조회는 같은 확정 H의 전체 Context를 검사한다. COMMITTED/VOID 영수증도
  전체 Context를 검사하므로 digest를 다시 계산한 잘못된 schema/contract/config/genesis를 거절한다.

OrderV1/BatchV2의 wire와 서명 domain은 유지한다. 서명에 결합한 genesis가 app_state의 contract/config/fee를
간접적으로 결합한다. 다른 fee genesis의 올바른 사용자 서명도 거절하고, fill fee version도 검사한다.
`C_start`, 양측 서명, rollback, 재생 추가 지급0, 직접 출금 등 기존 규칙은 유지한다.

## 재현

기존 설치 Go 1.26.5와 승인 dependency cache를 사용하며 다운로드/새 설치는 필요하지 않다.

```sh
cd chain/app
GOTOOLCHAIN=local GOPROXY=off go test -mod=readonly -tags dev_local_demo ./... -count=1 -json
GOTOOLCHAIN=local GOPROXY=off go test -mod=readonly . -run '^TestS3QueryCanonicalContextAndHistory$' -count=1
```

`NUS_S3_EVIDENCE_DIR=<별도 시험 경로>`를 지정하면 exact genesis, signed TxRaw/Batch,
높이·결과·exchange state, profile/manifest/guard 입력, 조회 원문과 restart 전후 응답을 기록한다.
공개 합성 fixture key는 서비스에서 사용하지 않는다. LevelDB는 새 시험 임시 경로에서 생성·닫는다.
기존 S1/S2 home·HELD_S2·표준 blocker를 수정하지 않는다.

최초 prefix 초기화 순서 오류와 재시험 결과는 Paperclip 보고서/증거에 구분한다.
새 후보는 CTO→Security 심사를 받아야 하며 Security 최종 판정에서 CEO에게 후속 조정을 인계한다.
