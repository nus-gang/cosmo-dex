# B → C 초기화 입력 전송

`chain/app/internal/localkeys.InputBundle`은 `ValidateLocalDemo`가 검증한 manifest/files/guard/genesis 원문을 C의 기존 `Validated::decode_bundle` 형식으로 직렬화한다. pin과 effective profile은 기존 C API처럼 별도 인자다. 입력은 호출 동안 단독 소유하며 결과는 독립 bytes이다. B 거절/48MiB 초과는 결과를 반환하지 않는다.

조직 승인·runtime pin을 발급하지 않는다. `Engine::create`에 필요한 bootstrap은 실제 신뢰 로컬 ChainSnapshot이어야 한다. genesis를 Snapshot으로 재해석하거나 합성 bootstrap을 실제 home에 사용하지 않는다. L-R에서는 이 입력 검증·컴파일을 준비하고 실제 체인 조회 및 서비스 기동은 승인 후 L-T에서 한다.

교차 시험: `NUS_C_VALIDATOR=<absolute offline nus-s3-local-demo> go test -mod=readonly -tags dev_local_demo ./internal/localkeys -run 'TestInputBundleRustValidation|TestPrepareGuard' -count=1 -v`. 명시한 validator가 없으면 교차 시험은 SKIP이다. 실행 근거에는 SKIP 여부를 기록한다. fixture descriptor/pin/사용자 공개키는 합성이며 runtime 승인이 아니다.
