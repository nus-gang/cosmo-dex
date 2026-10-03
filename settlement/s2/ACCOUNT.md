# S2 DIRECT 확정 계정 조회 — NUS-48

`GET /s2/accounts/{owner}` (`owner`: canonical `nus` bech32). 기존
`/s1/txs`, `/s1/txs/{HASH}`, `/s1/accounts/{owner}/requests/{request_id}`와 함께
S2 포트(기본 8788)에서 제공한다. Wallet/공통 protocol 변경은 없다.

## 공개/개인 경계

이 경로는 공개 체인의 등록된 두 계정 상태이며 Bearer 인증이 필요하지 않다.
Origin 없는 GET을 허용하고, 브라우저 Origin은 기존 localhost/127.0.0.1:5173만
허용한다. `Cache-Control: no-store`. `/s2/me` 및 주문·R/D/P·세션 정보는 기존
인증 경로로만 조회한다. 공개 조회는 개인 세션의 소유권 증명이 아니다.
서명 시 Wallet은 자기 공개키/owner 및 고정 context와 응답을 대조해야 한다.
매칭 엔진 승인·인증·가용성은 DIRECT 조회·TX 제출의 선행 조건이 아니다.

## 200 응답

- `state: "COMMITTED"`, `signing_ready: true`
- `context`: 기존 S2 snapshot의 chain_id/genesis_hash/contract_hash/config_hash/
  schema_version/market_id/market_config_version 전체
- `owner`: nus bech32, `owner_base64`: 같은 20바이트 owner
- `public_key_type: "/cosmos.crypto.mldsa65.PubKey"`, `public_key_base64`: 등록 ML-DSA-65 키
- `account_number`, `sequence`, `owner_epoch`: 서명에 사용할 확정 계정 값
- `balances`: DEVBASE, DEVQUOTE 순서의 `{denom, bank_atoms, confirmed_atoms}` 배열.
  `confirmed_atoms`는 체인 예치 C이며 주문 예약이나 잠정 수취 P를 포함하지 않는다.
  DIRECT 출금이 기존 주문과 경합하면 체인 epoch/원장 정정 규칙을 따른다.
- `gas_denom: "DEVGAS"`, `gas_atoms`
- `observed_height`, `cursor_height`: 같은 확정 높이 H
- `block_hash`, `block_time_unix_ms`, `freshness_ms`, `query_latency_ms`,
  `indexer_mode: "DIRECT_COMMITTED_QUERY"`

모든 정수는 부호/선행 0 없는 십진 문자열(u64)이다. `balances` 금액은 atoms.
개인키·서명·주문·세션·잠정 잔고는 응답에 없다. 금액/sequence를 합성하지 않는다.

## 거부와 최신성

등록되지 않은 주소는 404 `UNKNOWN_ACCOUNT`, GET 이외에는 405.
RPC 장애·context/키/owner/높이/블록 불일치·자산 보존 실패·stale·미래 시각·
지연은 503 `UNKNOWN_UNAVAILABLE`, `signing_ready:false`이며 계정 값은 반환하지 않는다.
query/fragment/임의 높이 선택은 이 DIRECT 경로가 아니다.

검증된 S2 Snapshot을 H로 고정 조회하고 블록 hash/time/chain을 대조한다.
키 SHA256 앞 20바이트를 등록 owner에 대조하고 두 자산의 bank+C 합을 genesis
공급량 및 module C에 대사한다. 모든 값은 같은 snapshot에서 가져온다.
응답 직전 status를 다시 읽어 H/chain/catching_up을 대조하며, 고정 profile의
freshness ≤5000ms, 미래 시각 ≤1000ms, 총 조회 지연 ≤2000ms를 적용한다.
조회 중 tip이 전진하면 보수적으로 503 후 새 조회한다. 서버는 잔고를 캐시하지 않는다.
응답 후 체인이 진행할 수 있으므로 `signing_ready`는 sequence 예약이나 성공 보장이
아니다. TX의 sequence/epoch/expiry/signature 최종 검증은 체인이 담당한다.

## 재현

```sh
S2_ENGINE_BINARY=/path/to/exchange-s2 python3 -m unittest discover -s settlement/s2 -v
# 기존 web/package-lock.json 의존성만 사용; 별도 node_modules 추가 없음.
# 승인 체인 7ce952475b4985096344d713467208722b5e7693의
# chain/app/scripts/build.sh로 빌드한 S2 지원 바이너리 및 설치된 Chromium 사용.
S2_TEST_SCRATCH=/private/run-dir S2_TEST_CHAIN=/path/to/nusd \
  CHROME_BIN=/path/to/chrome node settlement/s2/check_account_browser.mjs /path/to/evidence
```

`S2_TEST_WEB`으로 기존 설치된 web 의존성 디렉터리를 지정할 수 있다.
브라우저 CSPRNG 비밀키는 브라우저 메모리 밖으로 나가지 않는다. 공개키로 새 genesis를
만들고 두 사용자×두 자산 각각 예치·출금(8 TX)을 서명한다. REST 원시 응답,
서명 TX, 체인 inclusion/receipt, sequence/epoch/bank/C/GAS 변화 및 실제 RPC 중단
503을 기록한다. 시험 서버는 매칭 엔진을 실행하지 않는다. 단일 검증인 시험이며
Wallet 제품 브라우저 인수·4검증인 통합 QA를 대신하지 않는다.
