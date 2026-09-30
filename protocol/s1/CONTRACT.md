# S1-A 실행 계약 v1.0.0-rc1

2026-09-30 · CTO · NUS-18 · Security → QA 심사 후보. 승인 Plan revision `ab2893a6-b4bf-4497-938d-665c554bdb11`, S0 기준선 `df4da26463824de4f9450a478047d3c07d8fc308`.

## 범위와 고정 버전

이번 결정은 DEVQUOTE 하나, 테스트 사용자 2명, 로컬 동일 투표권 검증인 4개, 예치/직접 출금에 한정한다. DEVGAS는 가스 전용이다. 주문/매칭/배치 정산/송금/발행/후원/브리지/영속 지갑 복구/유료 자원은 활성화하지 않는다. S0 원본 `protocol/v1`과 기존 T01~T16 조건은 그대로 보존한다.

앱 명칭 `nusd`, 신규 구현 위치 `chain/app`(별도 Go module), 앱 계약 버전 `s1-dev-1`. 이 문서는 앱 바이너리나 실행 SHA를 만들어 낸다는 뜻이 아니다. 실제 앱 구현 NUS-19와 개발망 NUS-20이 이 계약을 소비한다.

| 항목 | 고정값 | 공식 근거 |
|---|---|---|
| Cosmos SDK | v0.55.0, commit `64fd208a11fb54f7ffdca1a1290c2cfbbc254e49` | [go.mod](https://github.com/cosmos/cosmos-sdk/blob/v0.55.0/go.mod) |
| CometBFT | v0.40.0, commit `0880b4d378f347ab16e54ec677ff50d803f37d62` | [go.mod](https://github.com/cometbft/cometbft/blob/v0.40.0/go.mod) |
| 앱 Go | 1.26.5, GOTOOLCHAIN=local | SDK go directive 1.26.5, Comet 최소 1.25.0 |
| ML-DSA | SDK mldsa65 → Comet mldsa65 → CIRCL v1.6.3 | [SDK key.go](https://github.com/cosmos/cosmos-sdk/blob/v0.55.0/crypto/keys/mldsa65/key.go) |
| S0 Go | 기존 1.24.4 유지 | S0 `chain/go.mod`를 앱 module로 바꾸지 않는다 |

공식 태그 소스 사본과 SHA256은 evidence 및 manifest.json에 고정한다. SDK/Comet 코어 LICENSE 사본을 보존한다. enterprise/poa·enterprise/group은 import하지 않는다. 의존성 선언의 호환성 확인이며 앱 빌드·네트워크 실행 성공은 아니다. 앱 전체 go.sum/MVS 결과, 실제 빌드된 모듈 목록은 NUS-19에서 `go list -m all`, `go version -m`로 대조하여 기록한다. 그 결과가 위 직접 의존성 pin과 다르면 조용히 교체하지 말고 CTO 계약 변경 심사를 거친다.

Go 공식 배포 SHA256: darwin-arm64 `efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a`, linux-amd64 `5c2c3b16caefa1d968a94c1daca04a7ca301a496d9b086e17ad77bb81393f053`. 출처 [go.dev 다운로드 목록](https://go.dev/dl/?mode=json&include=all).

## 계정·서명

두 공개 테스트 키만 genesis BaseAccount에 raw 공개키와 함께 미리 등록한다. 등록 계정의 account_number/sequence를 확정 높이 조회로 얻으며 임의 0 기본값을 넣지 않는다. account_number는 genesis 생성 결과를 사용한다. raw key=1952 bytes, signature=3309 bytes, 주소=SHA256(raw public key)[:20], 표시 HRP=nus의 lowercase Bech32. PubKey Any URL은 `/cosmos.crypto.mldsa65.PubKey`, 내부 bytes key tag1. 실제 키 등록값과 signer type/raw bytes/address가 모두 같아야 한다. 다른 키 유형, 미등록 계정, 키 교체는 S1에서 거절한다. 합의/P2P 키는 Ed25519이며 체인 전체 PQ 보장을 주장하지 않는다.

체인 TX는 한 메시지·한 signer·한 signature만 허용한다. SDK `SIGN_MODE_DIRECT=1`; 서명 입력은 SDK protobuf `SignDoc{body_bytes,auth_info_bytes,chain_id,account_number}` 직렬화 결과 그대로다. ML-DSA-65 pure, 빈 context, 추가 SHA256 prehash/ORDER frame을 붙이지 않는다. TxRaw(body_bytes tag1, auth_info_bytes tag2, signatures tag3)의 SHA256 uppercase hex가 TX hash다. 결정적 서명은 테스트 벡터 생성에만 요구하며 검증은 유효 서명을 모두 받는다.

SDK envelope는 일반 proto3 0 omission 규칙을 사용한다. S0 strict wire의 zero presence 규칙을 SDK envelope에 적용하지 않는다. 새 메시지의 모든 string/bytes 필드는 비어 있지 않아야 한다. SDK decoder 외에 메시지 필드 canonical validation을 수행한다. owner는 SDK signer annotation에 연결한다. fee payer/granter는 비어 있고 실제 payer는 owner; feegrant/authz/multisig/unordered/extension options/tip/비어 있지 않은 memo를 거절한다. fee denom DEVGAS, gas_limit/fee를 서명하고 사용자가 확인한다. max TxRaw=16,384 bytes, message 1개. 서버는 서명 이후 본문·fee·sequence를 변경할 수 없다.

## 메시지와 상태 전이

새 type URLs: `/nus.exchange.v1.MsgDeposit`, `/nus.exchange.v1.MsgWithdraw`. 정확한 tag/type은 messages.proto. 두 메시지 모두 owner, denom, amount_atoms, request_id, expected_epoch, expiry_height, genesis_hash를 포함한다. denom은 DEVQUOTE; amount_atoms는 `[1-9][0-9]*`, 1..10^12 atoms이며 U128 검사; expected_epoch/expiry_height는 정규 U64 십진 문자열. request_id는 32 bytes, genesis_hash는 32 bytes. 표시 decimals=6, 부동소수·반올림 금지. expiry는 실행 높이 h < expiry_height, 등호부터 EXPIRED. TX timeout_height=0으로 두고 앱의 exclusive expiry만 적용한다.

허용 체인 ID `nus-s1-dev-1`. 시작 전 실제 genesis 파일 bytes SHA256을 앱의 불변 실행 설정과 Wallet/REST network manifest에 주입한다. genesis 안에 자신의 hash를 넣지 않는다. 모든 검증인이 동일 hash를 사용해야 하며 누락/불일치 시 시작을 거절한다. 재생은 같은 genesis bytes와 설정을 요구한다. 메시지 genesis_hash와 SignDoc chain_id를 둘 다 검사한다.

B[u]=bank DEVQUOTE, C[u]=exchange 확정 채권, U=exchange module bank DEVQUOTE, E[u]=owner epoch. 초기 C=U=E=0. 신규 예치 a: B[u]-=a; U+=a; C[u]+=a; E 불변. 신규 출금 a: C[u]-=a; U-=a; B[u]+=a; E[u]+=1. 출금 수취인은 owner 고정이며 별도 recipient 없음. epoch U64 overflow는 전체 메시지 거절. 예치와 출금 모두 expected_epoch==E[u]를 요구한다. 출금의 epoch 증가·채권 차감·bank 이동·영수증 저장은 하나의 SDK message cache commit이다. admin/운영자 서명만으로 출금 불가. 서버를 거치지 않는 사용자 직접 TX도 동일 처리한다.

보존식: `U=sum(C[u])`, `sum(B[u])+U=genesis DEVQUOTE supply`, 모든 B/C/U>=0 및 U128 범위. 거래 수수료 T/미배정 채무는 S1에서 0; R/D/P는 아직 연결되지 않았으므로 REST 잔고에 합치지 않는다. 가스는 별도 DEVGAS supply 원장으로 fee collector·기타 module 보유분까지 합산한다. DEVQUOTE 공급량은 실행 중 mint/burn 불가. 은행 MsgSend/MultiSend·authz 우회로 exchange module에 직접 입금하는 경로를 거절한다. SDK 내부 bank keeper의 인증된 예치/출금 이동만 허용한다. 외부 module 직접 송금으로 U!=sum C가 되면 성공으로 숨기지 않는다.

## 재시도·가스·영수증

영구 키 `(genesis_hash,owner,request_id)`는 예치/출금 공통 namespace. 값은 type_url+canonical message bytes에 결합한다. hash=SHA256(u32be(type_url length)||ASCII type_url||u64be(message length)||message bytes). 같은 ID·다른 본문/종류는 ID_CONFLICT. 성공한 기존 ID·동일 본문은 기존 receipt를 돌려주고 DEVQUOTE와 epoch를 다시 변경하지 않는다. 조회 순서: resource/encoding/context/auth → 기존 receipt/ID conflict → 신규 epoch/expiry/amount/balance → 원자 commit. 따라서 출금 성공 후 old expected_epoch 및 만료가 되어도 인증된 동일 요청 재조회는 성공 영수증을 보존한다. receipt/tombstone GC 없음.

SDK account sequence와 앱 request_id는 별개다. 동일 TxRaw 재전송은 이미 소비된 sequence로 CheckTx 실패할 수 있다. timeout이면 TX hash 및 request_id 조회 후 동일 bytes를 우선 재전송한다. 새 account sequence로 재서명이 필요하면 사용자 서명을 다시 받아 같은 메시지/request_id를 전송한다. 기존 성공 조회를 새 자산 이동으로 표시하지 않는다. 새로운 ID를 자동 생성하여 모호한 출금을 반복하지 않는다.

Ante 성공 후 메시지 실패는 DEVQUOTE·epoch·receipt를 rollback하지만 가스와 account sequence는 SDK 처리에 따라 소비될 수 있다. 실패·이미 성공한 요청의 재서명 TX에도 가스 비용이 생길 수 있다. 불확실 제출 상태에서 DEVGAS가 무료/환불이라고 표시하지 않는다. `REJECTED_FINAL`은 한 TX의 확정 실패이며 동일 요청의 다른 in-flight TX 실패를 증명하지 않는다.

## REST 소비 계약

모든 정수는 JSON 십진 문자열, hash는 lowercase hex(Comet TX hash만 uppercase hex), key/TxRaw는 canonical padded base64. 로컬 HTTP origin만 허용된 개발망이며 로그인·영속 키 백업은 만들지 않는다. 서버에는 개인키·seed를 보내지 않는다. 총 상태 조회는 하나의 committed height에서 수행하며 높이가 섞이면 503 SNAPSHOT_UNAVAILABLE.

| 경로 | 입력/출력 및 의미 |
|---|---|
| GET /s1/network | chain_id, genesis_hash, contract_version, denom, decimals, gas_denom, app_version, observed_height; hash 미고정 시 503 |
| GET /s1/accounts/{owner} | owner, public_key_type, public_key_base64, account_number, sequence, epoch, bank_atoms, exchange_atoms, gas_atoms, observed_height, state=COMMITTED |
| POST /s1/txs | 입력 오직 `{tx_bytes:base64}`; 서버가 SHA256 계산; 202 `{tx_hash,state:SUBMISSION_UNKNOWN,check_tx_code,observed_height}`; CheckTx code 0도 확정 아님 |
| GET /s1/txs/{tx_hash} | `{state,tx_hash,height,code,codespace,gas_wanted,gas_used}`; inclusion 및 result code=0 확인 후 COMMITTED, code!=0이면 REJECTED_FINAL |
| GET /s1/accounts/{owner}/requests/{request_id} | 200 immutable receipt; 없으면 404 `{state:NOT_FOUND_AT_HEIGHT,observed_height}`. 404는 실패 증명 아님 |

필수 receipt: chain_id, genesis_hash, owner, request_id(hex64), request_hash(hex64), operation(DEPOSIT/WITHDRAW), denom, amount_atoms, committed_height, original_tx_hash, epoch_before, epoch_after, state=COMMITTED. receipt는 최초 성공 TX를 보존한다. 상태 조회의 잔고는 별도 observed_height이며 receipt에 현재 잔고처럼 섞지 않는다. 중복 TX의 성공 이벤트는 original_tx_hash를 참조한다. 이벤트 유실은 committed storage 조회로 복구한다.

공통 오류 body `{code,retryable,state,observed_height}`. parse/context/key/signature/ID/epoch/expiry/amount/balance 오류는 retryable=false. 네트워크 timeout/disconnect/503은 SUBMISSION_UNKNOWN, retryable=true. bank 잔고 오류=INSUFFICIENT_BANK_BALANCE, exchange=INSUFFICIENT_CONFIRMED_BALANCE, epoch=EPOCH_MISMATCH, malformed amount=INTEGER_RANGE 또는 NON_CANONICAL_INPUT, unsigned owner mismatch=UNAUTHORIZED. 원시 SDK code/codespace는 별도 필드에 보존한다. HTTP 상태나 tx indexer 존재만으로 확정하지 않는다. indexer off/lag면 receipt 및 committed 높이를 직접 조회하고 계속 UNKNOWN을 유지할 수 있다.

## 담당별 소비 경계·추정

아래는 CTO 최초 작업량 추정(집중 작업일), 담당자가 확약한 가용량이 아니다. 모든 담당 실제 병렬 슬롯/일별 가용량은 미확인. 인력 증원·유료 자원 가정 없음. 09-30 착수/10-07 검토는 체크포인트이며 납기 보장 아님. 첫 실제 TX 확정 후 CEO가 재추정한다.

| 담당/업무 | 인수 입력 → 산출 경계 | 추정 / 가용량 |
|---|---|---|
| CTO NUS-18 | 계약·벡터·pin → Security/QA 심사; 변경 승인 | 1–2일 / 이번 heartbeat 수행 |
| Chain NUS-19 | 이 계약 → 앱/ante/keeper/receipt/Go lock/실제 TX | 3–5일 / 미확인 |
| SRE NUS-20 | 앱 → genesis/config/hash/4 validator/CI/CLI 안내 | 2–3일 / 미확인 |
| Settlement NUS-21 | committed query/TX → 위 REST·유실 복구; batcher 비활성 | 2–3일 / 미확인 |
| Wallet NUS-22 | network/account/SignDoc → 테스트 서명·브라우저 입출금 | 2–4일 / 미확인 |
| Security NUS-23 | 정확한 PR head → 독립 권한/보존/재시도 판정 | 1–2일 / 미확인 |
| QA NUS-24 | reviewed main SHA → 새 checkout/AT01~07/시연 | 1–2일 / 미확인 |
| Exchange | 계약의 epoch/확정 잔고 소비 경계만 기록; 구현 미배정 | 0일 S1 구현 / 해당 없음 |

Chain→SRE→실제 REST→Wallet 통합이 주 경로이며 Security/QA는 해당 결과를 검토한다. 별도 장기 실행 대기/폴링을 만들지 않는다. 이미 배정된 업무의 blocker를 사용한다. Exchange의 미래 R/D/P 재생은 epoch 변경에 반응하되 S1 성공을 주문 ACK 재생·출금/정산 경합 검증으로 확대하지 않는다.

## 결정 기록·회고

ADR S1-A-01: S0 계약과 코덱/서명 벡터는 수정하지 않고 `protocol/s1`에 SDK TX 계약을 추가한다. DEC-07의 S0 서명 프레임과 SDK envelope 차이를 명시하여 중복 정의를 피한다. DEC-02/03/04/05/09/10의 S0 결정은 보존; 활성 입출금은 위 S1 신규 request_id/epoch/expiry/정수 규약을 따른다. 송금·거래 fee·가격 규약 변경 없음. DEC-01/06/08은 개발망 테스트 공급·검증인·자가 가스만 결정하며 실운영 미결. DEC-11은 공개 테스트 데이터만, DEC-12 목표 TPS/RPO/RTO는 미검증. CTO가 계약 변경 owner, Chain/Wallet이 근거 담당, Security→QA가 독립 검토이며 목표 시점은 NUS-19 착수 전 계약 승인이다. 실운영 결정은 CEO 후속 승인 범위다.

S0 회고: rc2→rc3→rc4에서 JSON atoms 표현·snapshot binding/presence가 늦게 정렬되어 재시험을 유발했다. 이번에는 protobuf 기본값과 API presence·단계별 오류를 먼저 고정한다. 여러 후보 PR의 통합이 늦어 후보 검증과 main 인수가 분리되었다. 각 PR head와 main SHA 증거를 구분하고 CEO의 통합 경로를 따른다. reviewer와 executor 혼선은 심사자의 approve/request_changes, 원 executor/CEO의 merge로 분리한다. 구현자가 자신의 보안·QA 최종 승인을 하지 않는다.

## 검증·인수

`python3 protocol/s1/tools/check.py`: 합성 byte/hash·상태 기대값과 S0 무변경 확인. `vectors/direct.json`은 새 SDK envelope 및 입출금 wire fixture; `vectors/state.json`은 보존/재시도 기대값. 기존 S0 벡터 그대로. Go ML-DSA fixture 서명 검증은 별도 기록하며 실제 SDK ante/네트워크 실행과 구분한다.

| ID | 실제 제품 인수 조건 | 담당 |
|---|---|---|
| S1-AT01 | 같은 genesis hash 4 validator, 정확한 앱/Go/lock/config/실행 SHA; 3/4 commit | Chain/SRE |
| S1-AT02 | 두 등록 ML-DSA 계정에서 예치→출금, receipt/전후 B,C,U,E와 보존식 | Chain/QA |
| S1-AT03 | wrong owner/key/chain/genesis, 0/overflow/초과금액, epoch/expiry 경계, 동일/충돌 ID, sequence 재시도·가스 | Security |
| S1-AT04 | 앱/REST 재시작 후 잔고·receipt 동일, 유실 응답 재조회, 높이 일관성 | Chain/Settlement |
| S1-AT05 | 1 validator 중단 진행, 2 중단 확정 불가·UNKNOWN; 복구 후 중복 지급 없음 | SRE/QA |
| S1-AT06 | 브라우저 signer 로컬 유지, account/fee 확인, 서명→제출→확정 조회; UNKNOWN 표시 | Wallet/QA |
| S1-AT07 | protected main 반영·그 SHA의 CI·새 checkout 독립 QA·사용자 안내/시연 | CEO/QA |

A 단계에서 AT01~07은 모두 NOT_RUN. SDK/Go 조합의 앱 build 검증은 NUS-19, genesis/config의 실제 bytes hash는 NUS-20 산출이다. runtime manifest는 execution_sha, binary_sha256, sdk/comet versions, go_version, go_sum_sha256, genesis_sha256, 각 validator config_sha256, contract/vector hash, 실행 명령/환경, tx hashes/heights/results를 필수로 가지며 null/placeholder가 있으면 runtime 인수를 거절한다. 이 계약 심사 완료나 합성 벡터 PASS는 S1 완료가 아니다.

## 소스 불일치와 고정 개발 설정

공식 `sdk-keys.proto` 주석은 validator-only라고 쓰지만 같은 태그의 `sdk-UPGRADING.md` Account Keys 절과 `sdk-mldsa65.go` 및 `sdk-sigverify.go`는 계정 키 및 ante 지원을 명시한다. S1은 실제 구현과 해당 업그레이드 안내를 근거로 기존 raw20 주소를 채택한다. proto 주석을 수정하거나 ADR-28 파생으로 몰래 교체하지 않는다. Security는 이 불일치와 NUS-19의 실제 SDK account/TX 증거를 함께 검토한다.

개발 설정 고정: 사용자당 초기 DEVQUOTE=10^12 atoms, DEVGAS=10^9 atoms, 초기 C/E=0. 검증인 운영 계정 4개는 사용자 2개와 별도이며 DEVGAS stake/운영 배분은 NUS-20 genesis에 명시·해시 고정한다. 사용자 DEVQUOTE 공급은 총 2*10^12이며 기타 계정 DEVQUOTE는 0이다. gas 가격/상한과 검증인 staking DEVGAS 공급은 첫 실제 TX 측정으로 NUS-19/20이 설정 파일에 고정할 실행 파라미터다. direct fixture의 gas=500000/fee=1000은 암호 fixture 값이며 실제 적정 가스 주장이나 런타임 기본값이 아니다. 앱은 고정된 params/manifest 없이 런타임 인수를 받지 않는다.
