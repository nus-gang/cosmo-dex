# S3-A 근거·검증 경계

## 승인·실제 baseline

- [S3 실행 계획](/NUS/issues/NUS-53#document-plan), revision `ece8cc33-a2e6-41ec-9810-0e4095fb3016`; [A 담당 plan](/NUS/issues/NUS-54#document-plan) revision `f76e44ff-f7ec-4da1-8455-420aacea9704`. 새 확인 카드가 필요한 계획 변경 업무가 아니다. A의 계약 후보는 Security→QA가 심사한다.
- GitHub main을 재조회하고 독립 clone의 HEAD/tree를 대조했다: [`bd9e473196ac86fdedf655b2c93e6931f54faa83`](https://github.com/nus-gang/cosmo-dex/commit/bd9e473196ac86fdedf655b2c93e6931f54faa83), tree `31a0d1c65e9b71647cac6c4c45bf7e8d2dd9d7f3`. 공유 root HEAD `7942e9347f48d6322cf0e6412156cba7e3b5386f`와 다른 담당 worktree는 변경하지 않았다.
- [S2 QA 원문](/NUS/issues/NUS-45#document-qa-results), revision `e36782b3-18ef-4c48-88ad-ba5376c5b9e9`; [T01~T16 원문 기준](/NUS/issues/NUS-9#document-acceptance), revision `d4f05eaa-dd46-487b-a520-00a1cd9cda0d`를 읽었다. S2 main QA·최초 DIRECT_UNAVAILABLE·HTTP 실제경쟁·부분/NOT_RUN 추적을 보존한다.
- [개발 설계서](/api/attachments/9c378275-dcec-4402-8478-4bf7d26af911/content)를 재다운로드하여 SHA256 `587f3782a531256eac6545884e7f91377120db9d74fab130e931c5605573f0b4` 대조. 보존 추출문의 p6–12 자산·batch·출금·실패·상태/재생과 p14 T 기준을 우선 읽었다.
- [아키텍처](/api/attachments/d157936f-f4b3-45e4-849a-f68621a7b90d/content) 재다운로드 SHA256 `0e8e8ddd0484953339a2dee9964327436e79f018527870c69ba88e85315eb663`; 기존 보존 추출문과 대조했다. 장기 성능/분산 설계는 S3 local proof로 인수하지 않는다.

## 공식 고정 tag·실제 코드

실제 baseline lock은 SDK v0.55.0, store/v2.0.0, CometBFT v0.40.0, CIRCL v1.6.3이다. 버전/지원/라이선스의 기존 근거는 상속 `protocol/v1/evidence`, `protocol/s1/evidence`, `chain/app/evidence/licenses`, `exchange/evidence` 및 각 lock이다. 이번에 의존성 변경은 없다.

- SDK bytes당10gas/ML-DSA750gas는 [v0.55.0 auth params 공식 코드](https://github.com/cosmos/cosmos-sdk/blob/v0.55.0/x/auth/types/params.go)에서 확인하고 `sdk-auth-params.go.txt`에 보존했다. 이 가격은 지연/수수료의 실측 결과가 아니다.
- TX `h>timeout_height` 거절은 [v0.55.0 ante basic](https://github.com/cosmos/cosmos-sdk/blob/v0.55.0/x/auth/ante/basic.go), 보존 `sdk-ante-basic.go.txt`. v1 주문의 h>=expiry와 다른 등호를 fixture에 고정했다. baseline `chain/app/app.go` guard가 timeout0만 허용하므로 S3 handler의 명시적 변경을 B에 요구한다.
- read/write 고정·byte gas는 [store/v2.0.0 공식 gas.go](https://github.com/cosmos/cosmos-sdk/blob/store/v2.0.0/store/types/gas.go), 보존 `sdk-store-gas.go.txt`. 실제 B gas/KV 계측은 NOT_RUN이다.
- [CometBFT v0.40.0 consensus params](https://github.com/cometbft/cometbft/blob/v0.40.0/types/params.go)의 block bytes/gas·evidence·commit 여유를 확인했다. 기본 unlimited gas를 그대로 사용하지 않고 S3 profile에 수치를 고정한다. validators는 Ed25519이며 앱 ML-DSA34개와 혼동하지 않는다.
- fixture는 [CIRCL v1.6.3 ML-DSA65](https://github.com/cloudflare/circl/tree/v1.6.3/sign/mldsa/mldsa65)를 기존 chain/go.mod/go.sum으로 고정해 사용한다. 공개 seed/raw키 길이·순수 서명·mutation/prehash/context 거절을 실제로 실행한다. SDK 지원 전체나 Rust/TS의 새 통합 PASS를 주장하지 않는다.

## A 검사 구분

`crypto-benchmark.json`의 100 warmups + 1000 samples×34검증과 모든 원시 ns를 보존한다. 환경은 도구가 기록한 실제 Go/OS/arch다. 단일 머신 microbenchmark이고 블록 확정/가스·지속처리량·최소자원의 증거가 아니다. 상태 oracle은 명세 예제의 consistency와 false-positive 검출만 증명한다. 독립 Security/QA의 승인과 제품 G/J 시험은 별도다.

[verification.txt](verification.txt)·[verification.json](verification.json) 및 Paperclip 인계 문서가 실행 명령·결과를 기록한다. check.py의 manifest가 file hashes를 검산한다. runtime genesis/H/TX inclusion 값은 아직 존재하지 않으므로 null/NOT_RUN이며 합성 fixture 값을 실측란에 복사하지 않는다. actual S3-AT01~09는 acceptance.json의 모든 항목이 NOT_RUN이다.
