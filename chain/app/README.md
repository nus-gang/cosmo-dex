# S1 nusd 개발 앱

승인된 S1-A 계약(별도 PR #15, head d41be80fc325cf76b47b35b3f9b37359544e363e)을 소비한다. S0 chain 모듈은 보존한다. SDK v0.55.0 / CometBFT v0.40.0 / Go 1.26.5. 실제 BaseApp, auth/bank keeper, IAVL/GoLevelDB, CometBFT, ML-DSA-65 DIRECT 서명 경로다.

## 실행

Go 1.26.5를 PATH에 설정하고 chain/app에서:

```sh
sh scripts/build.sh
bin/nusd init --home ./bin/devnode --operator-accounts ./config/operator-accounts.json > ./bin/init.json
# 출력의 실제 genesis_hash를 사용한다. 임의 기본 hash 없음.
bin/nusd start --home ./bin/devnode --genesis-hash <init의_hash>
```

별도 터미널:

```sh
bin/nusd snapshot
bin/nusd tx --user 0 --op deposit --amount 1000000 --request-id 0000000000000000000000000000000000000000000000000000000000000001
bin/nusd tx --user 0 --op withdraw --amount 400000 --request-id 0000000000000000000000000000000000000000000000000000000000000002
bin/nusd receipt --user 0 --request-id 0000000000000000000000000000000000000000000000000000000000000002
```

`--user 1`은 두 번째 사용자다. 공개 합성 seed `[1,0,...]`/`[2,0,...]`이며 실자산 사용 불가. 사용자 키를 서버로 전송하지 않고 CLI 내부에서 서명한다. account_number/sequence/epoch/genesis hash는 committed snapshot에서 조회한다. 금액/수수료는 정수 atoms. `--out tx.raw`로 서명만 하고 `broadcast --file tx.raw`로 동일 bytes를 재제출할 수 있다. 자동 새 request ID 생성이나 자동 재서명은 하지 않는다. RPC timeout은 확정 실패가 아니며 hash/receipt를 조회해야 한다.

`start`는 home/config/config.toml을 읽는다. `--rpc`, `--p2p`는 명시적으로 전달한 경우에만 설정을 덮어쓴다. SRE가 동일 genesis에 4개의 Ed25519 validator와 peer 설정을 구성할 수 있다. `init`은 단일 검증인 smoke용이며 4검증인 AT01/장애 AT05 통과를 의미하지 않는다. genesis는 두 ML-DSA 사용자 각각 DEVQUOTE 10^12, DEVGAS 10^9, exchange=epoch=0이다. 합의 검증인은 정적 genesis 집합이며 staking/governance 메시지는 등록하지 않는다. 운영 4계정은 아래 명시적 genesis 입력을 사용하며 거래 signer가 아니다.

## 상태·API 경계

SDK gRPC query를 ABCI RPC로 호출한다:

- `/nus.exchange.v1.Query/Snapshot`: QuerySnapshotRequest → QueryJSONResponse.json. 한 SDK query context의 committed 높이에서 두 사용자 계정 B/C/E/sequence/key, 별도 operator_accounts의 초기/현재 DEVGAS와 DEVQUOTE=0, module DEVQUOTE, fee collector DEVGAS, 각 공급량과 genesis_gas_supply를 반환한다.
- `/nus.exchange.v1.Query/Receipt`: QueryReceiptRequest(owner, request_id hex) → immutable receipt JSON. 미존재 오류는 관측 높이를 포함하며 확정 실패가 아니다.
- `broadcast_tx_commit`: CheckTx, tx_result 및 inclusion height를 구분한다. CLI exit 0은 양쪽 code=0과 height>0일 때만 허용한다. Settlement의 `/s1/*` REST는 별도 담당 범위다.

허용 MsgDeposit/MsgWithdraw만 등록한다. SDK ante가 DIRECT 서명·account_number/sequence·가스를 처리하고 앱 guard가 등록 ML-DSA key=owner, one message/signer/signature, fee payer/granter 없음, memo/extension/unordered/tip 없음, size<=16384를 강제한다. exchange keeper의 메시지 cache에서 bank 이동·확정 채권·epoch·receipt를 원자 반영한다. 실패 시 SDK ante의 가스/sequence 소비와 메시지 rollback을 구분한다.

request ID namespace는 owner+ID이며 DB 전체가 immutable genesis hash에 결합된다. 동일 canonical message 요청은 기존 receipt를 보존하고 epoch/expiry 재검사를 생략한다. 다른 메시지 종류/본문은 ID_CONFLICT. 출금은 owner에게만 반환하고 epoch U64 overflow를 거절한다. 금액은 1..10^12 canonical 정수다. 모듈 보관=채권 합, bank 총잔고=각 자산 공급량을 검사한다. S0의 U(미배정)와 S1의 module 보관량은 다른 정의다.

## 가스·검증

단일 노드 첫 실제 예치 gas_used=253482, 출금=245235를 관측했다. 개발 기본 gas_limit=500000, fee=1000 DEVGAS; 최소 가격 1/500 DEVGAS per gas(정수 ceil), 최대 gas_limit=10000000. 부하/운영 적정성 보장 아님. 총 DEVGAS는 사용자+운영 4계정+fee collector에서 대사하며 DEVQUOTE와 섞지 않는다.

```sh
sh scripts/check-toolchain.sh
go test -mod=readonly -v ./...
go test -mod=readonly -race ./...
```

기본 시험은 실제 SDK FinalizeBlock/Commit 호출로 signature/owner/chain/genesis, 금액/epoch/expiry, 동일 TxRaw replay, 새 sequence 동일 요청, ID conflict, 보존식, 실패 메시지의 ante 소비/rollback을 확인한다. 이는 독립 reviewer 승인이나 4검증인 합의 시험을 대신하지 않는다. `evidence/first-confirmation.json`은 최초 탐색 바이너리의 실제 합의 증거이며 최종 소스 commit의 증거와 구분한다.

라이선스: 코어 SDK/CometBFT Apache-2.0, Go BSD-3-Clause, CIRCL BSD-3-Clause의 원본을 evidence/licenses에 보관한다. enterprise 모듈은 import하지 않는다. go.sum과 `go version -m`/모듈 목록으로 실제 빌드 의존성을 기록하며 전체 전이 의존성의 법률 검토 완료를 주장하지 않는다.

## 운영 계정 genesis 입력 (R1)

`init --operator-accounts <JSON 파일>`이 필수다. 파일은 정확히 4개의
`{"address":"nus1…","gas_atoms":"1000000000"}` 객체를 담은 배열이다.
동일 배열이 genesis의 `app_state.operator_accounts`에 들어가므로 SRE는
Comet validator 집합과 함께 실제 genesis bytes/hash를 고정할 수 있다.
SDK 운영 계정 주소와 Comet 합의 키는 별개다. 운영 계정은 pubkey 없는
BaseAccount이며 거래 signer 등록·키 교체·staking은 활성화하지 않는다.

주소는 lowercase nus Bech32(raw20), 사용자/운영 주소 중복 및 사용 중인 module/authority
주소 충돌을 거절한다. `gas_atoms`는 양의 정규 십진 **문자열**이며 U64 범위를
검사한다. 사용자 DEVGAS 2*10^9와 운영 배분 합도 U64를 넘을 수 없다.
누락/0/음수/선행 0/소수/숫자 JSON/중복 필드/미지 필드(예: quote_atoms)를 거절한다.
입력 검증 실패 시 init은 노드 home을 생성하지 않는다.
DEVQUOTE 배분 필드는 없으며 총공급 2*10^12를 보존한다.

snapshot의 `accounts`는 계속 두 사용자만 포함한다. 운영 원장은
`operator_accounts`에 분리하며 각 `exchange_signer=false`다.
genesis DEVGAS 공급은 저장소에 고정하고 이후 bank 공급 및 전체 잔고 합과 대사한다.
운영 잔고는 사용자 거래 및 재시작 후 그대로 유지되어야 한다.
이전 R1 미적용 genesis/DB의 마이그레이션은 제공하지 않는다. 개발망은 새 home과
검증된 운영 입력으로 초기화해야 하며 genesis hash를 바꿔 기존 DB를 재사용할 수 없다.

실제 단일 노드 입출금 및 운영 원장 재시작 시험:

```sh
python3 scripts/smoke.py --binary bin/nusd --output <새_증거_디렉터리> --operator-accounts config/operator-accounts.json
```

배분 합의: [NUS-26](/NUS/issues/NUS-26#document-operator-genesis-agreement)
revision `384211b6-c795-46ec-9c53-04d7d4ca0bff`. 저장소의
`config/operator-accounts.json`은 합의한 각 10^9, 총 DEVGAS 6*10^9 profile이다.
다른 형식상 유효한 금액은 이 개발 profile 인수가 아니며 계약·설정·hash 변경이 필요하다.
4개 공개 주소는 ASCII `nus-s1-dev-1/operator/mock/{i}` (i=1..4)의 SHA256
앞 20 bytes를 nus Bech32로 인코딩했다. private/public key를 생성하지 않은
배분용 합성 주소이며 서명 가능한 운영 지갑이나 비상 가스 사용을 입증하지 않는다.

### R2: genesis 필드명 경계

`DecodeGenesis`는 상위 `public_keys`/`operator_accounts`, 운영 계정의 `address`/`gas_atoms`만 정확한 대소문자로 허용한다. case alias, 추가 필드, JSON escape로 표현한 중복 키도 거절한다. CLI `init`은 이 검증을 home 생성 전에 수행한다.

```sh
python3 scripts/genesis-input-test.py --binary bin/nusd --operator-accounts config/operator-accounts.json --output /tmp/nus-genesis-input-tests
```

이 시험은 잘못된 입력 16개가 실패하고 home을 생성하지 않음을 검증한다. 정상 초기화·거래·재시작은 기존 `scripts/smoke.py`로 검증한다.
