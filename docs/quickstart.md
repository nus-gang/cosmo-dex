# S1 사용자 시작 안내

[문서 목차](README.md) · [운영](../ops/s1/README.md) · [API·서명 안내](api.md)

**현재 S0 공통 기반과 S1 로컬 테스트 자산 입출금 기능이 완료됐습니다.** 사용자는 웹 화면에서 두 시험 계정을 만들고, 실제 로컬 체인에 DEVQUOTE를 예치한 뒤 확정 잔고를 조회하고 자기 지갑으로 출금할 수 있습니다.

2026-09-30 문서 기준. DOC-1 시작 시 remote main을 조회한 결과 검증된 커밋 `32781aa97d62ec747e7a25c10fdb8b58030d79f2`와 같습니다. 아래 안내는 재현을 위해 이 커밋을 고정합니다.

## 사용자에게 제공되는 기능

| 기능 | 직접 할 수 있는 일 | 범위와 주의점 |
|---|---|---|
| 시험 지갑 | 두 계정 생성, 주소 확인, 계정 전환 | 키는 생성한 브라우저 탭 메모리에만 유지됩니다. |
| 잔고 조회 | 지갑 DEVQUOTE, 거래소 확정 잔고, 가스 잔고 조회 | 사용자가 ‘확정 잔고 조회’를 눌러 갱신합니다. 자동 갱신은 없습니다. |
| 예치 | 자기 지갑의 DEVQUOTE를 자기 거래소 잔고로 이동 | 실제 서명 TX와 로컬 체인 확정 결과를 확인합니다. |
| 출금 | 거래소의 확정 DEVQUOTE를 같은 자기 지갑으로 회수 | 외부 거래소·은행 출금 또는 타인 주소 송금 기능은 아닙니다. |
| 거래 결과 조회 | 예치·출금 TX hash, 확정 높이, 대기/확정/실패/확인 불가 상태 확인 | 주문·매매 내역이 아니라 예치·출금 내역입니다. |
| 오류·지연 확인 | 잔고 초과 사전 거절, 조회 지연·단절 경고 | 결과 불명확 시 새 TX를 자동 생성하지 않고 원래 hash로 조회합니다. |
| 세션 종료 | 미확인 TX 조회 후 세션 키 폐기 | 새로고침·탭 종료·reset 후에는 이전 키를 복구할 수 없습니다. |

구현 기반으로 실제 Cosmos SDK/CometBFT 앱, 한 호스트의 4검증인, ML-DSA 사용자 TX 서명, 영속 원장, REST 조회·제출, 재전송 중복 방지가 연결됐습니다. 노드/API 재시작 후 잔고 보존, 1개 검증인 정지 시 진행·2개 정지 시 확정 중단·복귀 후 재개도 S1 시험에서 검증됐습니다. 노드 데이터 보존과 브라우저 키 복구는 별개의 기능입니다.

기존 인수 결과는 main CI 8개 workflow·9개 job 성공, 독립 QA S1-AT01~07 PASS, CTO→QA→Security 최종 승인입니다. 이 수치는 기존 인수 증거이며 DOC-1의 새 검증 결과와 구분합니다. 출처는 [버전·검증 기록](verification.md)을 확인하세요.

## 준비할 환경

- macOS 또는 Linux, 저장소 읽기 권한이 있는 Git, **Go 정확히 1.26.5**, Python 3.10 이상, Node.js 22.18 이상과 npm, Chrome. 저장소 CI pin은 Node 24.21.0이다.
- Go 빌드 스크립트는 1.26.5를 정확히 검사합니다. 더 최신 Go를 설치했다는 이유만으로 통과하지는 않습니다.
- 의존성 다운로드 인터넷 연결과 Go/npm 의존성·4개 노드 DB 저장 공간이 필요합니다. QA는 64 GiB 호스트에서 성공했지만 최소 RAM/디스크 요구량은 측정되지 않았습니다. 64 GiB가 필수라는 뜻은 아닙니다.
- 빈 로컬 포트: 웹 8080, REST 8787, 검증인 P2P/RPC 28656/28657, 28666/28667, 28676/28677, 28686/28687.
- 이 S1 수동 시연은 Rust·Docker·MetaMask·Paperclip 토큰이 필요하지 않습니다. DEVQUOTE/DEVGAS는 genesis에서 공급하는 테스트 자산입니다.

아래는 **사용자 PC에서 실행하는 안내**입니다. 웹·개발망·API용 터미널 3개를 유지합니다. 같은 PC에서 Chrome을 열고 URL은 `localhost` 대신 정확히 `http://127.0.0.1:8080`을 사용하세요. 새 폴더 `~/cosmo-dex-s1`이 이미 있다면 기존 폴더를 지우지 말고 다른 이름을 정해 모든 명령의 경로를 맞추세요.

## 1. 터미널 A — 소스 설치·빌드·웹 실행

```sh
cd ~
git clone --branch main https://github.com/nus-gang/cosmo-dex.git cosmo-dex-s1
cd ~/cosmo-dex-s1
git checkout --detach 32781aa97d62ec747e7a25c10fdb8b58030d79f2

go version
node --version
npm --version
python3 --version

(cd chain/app && sh scripts/build.sh)
(cd web && npm ci --ignore-scripts && node s1/build.mjs)
(cd web && S1_API=http://127.0.0.1:8787 node s1/serve.mjs)
```

이 터미널은 계속 켜 둡니다. `S1 wallet: http://127.0.0.1:8080`이 표시되면 Chrome에서 해당 주소를 엽니다. S0용 `web/dist/demo.html` 대신 이 S1 주소를 사용합니다.

화면에서 **두 시험 계정 생성**을 누릅니다. **genesis 준비용 공개키 배열 (비밀 없음)**을 펼쳐, 표시된 `[`부터 `]`까지 JSON 배열 전체를 텍스트 편집기로 `~/cosmo-dex-s1/users.json`에 저장합니다. 파일 내용은 공개키 문자열 2개의 배열이며 Markdown 코드펜스나 객체 wrapper를 붙이지 않습니다. 이 공개키 파일은 지갑 백업이 아닙니다.

**이후 같은 탭을 유지하세요. 새로고침하거나 닫지 마세요.** 아직 개발망·API가 실행되기 전이므로 이 시점에 잔고 조회가 안 되는 것은 정상입니다.

## 2. 터미널 B — 공개키로 새 개발망 초기화·기동

```sh
cd ~/cosmo-dex-s1
python3 -m json.tool users.json
python3 ops/s1/devnet.py init --home .runtime/wallet --user-public-keys users.json
python3 ops/s1/devnet.py serve --home .runtime/wallet
```

`init`은 새 개발망에서만 한 번 실행합니다. 두 계정에 테스트 자산을 배분하고 동일 genesis를 쓰는 4개 노드를 준비합니다. 기존 `.runtime/wallet`이 있다면 덮어쓰지 않습니다. 이 터미널도 계속 켜 둡니다.

## 3. 터미널 C — 체인 확인·API 기동

```sh
cd ~/cosmo-dex-s1
python3 ops/s1/devnet.py health --home .runtime/wallet

S1_GENESIS_HASH="$(python3 -c 'import json; print(json.load(open(".runtime/wallet/manifest.json"))["genesis_sha256"])')"
echo "$S1_GENESIS_HASH"

python3 settlement/s1/server.py \
  --rpc http://127.0.0.1:28657 \
  --genesis-hash "$S1_GENESIS_HASH" \
  --journal .runtime/wallet/txs.sqlite \
  --port 8787 \
  --origin http://127.0.0.1:8080
```

`health`에서 노드 0~3에 오류가 없고 양수 높이·`catching_up: false`가 나오는지 확인합니다. 시작 직후라면 잠시 뒤 같은 health 명령을 다시 실행합니다. 이 확인은 기본 기동 확인이며 같은 높이의 app hash 비교 등 전체 QA를 대체하지 않습니다.

터미널에 출력된 64자리 hash를 열린 지갑 화면의 **genesis SHA256**에 붙여 넣고 **genesis 고정**을 누릅니다. 개발망 manifest·REST API·화면이 같은 hash를 사용해야 합니다. 터미널 C도 계속 켜 둡니다.

## 4. 브라우저 — 사용자 시연

| 순서 | 조작 | 확인할 결과 |
|---|---|---|
| 1 | 계정 ‘시험 사용자 1’ → ‘확정 잔고 조회’ | 주소, 지갑·거래소·가스 잔고, 조회 높이 표시. 새 개발망의 거래소 잔고 0 |
| 2 | 동작 ‘예치’, 금액 `100` → ‘위 조건으로 로컬 서명·제출’ | 제출 내역과 원래 TX hash 표시 |
| 3 | ‘TX 결과 조회’ | 해당 내역이 ‘확정’, 높이가 양수, 거래소 확정 잔고 `100` |
| 4 | 동작 ‘내 지갑으로 출금’, 금액 `40` → 서명·제출 → ‘TX 결과 조회’ | 출금 확정, 거래소 잔고 `60`, 지갑 잔고는 출금 전보다 40 증가 |
| 5 | 출금 금액 `60.000001` → 서명·제출 | `INSUFFICIENT_BALANCE`, 새 출금 TX 없이 잔고 `60` 유지 |
| 6 | ‘시험 사용자 2’로 바꿔 잔고 조회 후 1~5 반복 | 서로 다른 주소, 계정별 독립 잔고, 두 계정 모두 거래소 잔고 `60` |

확정까지 시간이 걸리면 **TX 결과 조회**를 다시 누릅니다. 제출 성공·HTTP 200만으로 확정된 것은 아닙니다. ‘확인 불가’인 경우 새 출금을 만들지 않고 원래 TX 결과를 다시 조회합니다. 화면을 새로고침해 해결하려 하지 마세요.

화면 금액은 DEVQUOTE 단위입니다. `100`을 입력하면 됩니다. 내부 정수 단위는 1 DEVQUOTE = 1,000,000 atoms이므로 100/40/60은 각각 100000000/40000000/60000000 atoms입니다. CLI의 정수 금액을 화면에 그대로 넣지 마세요. 가스는 별도 DEVGAS이며 화면에 수수료 1000 atoms·gas limit 500000이 표시됩니다. 실패에도 가스가 소비될 수 있습니다.

선택적으로 각 계정의 남은 `60`을 출금해 거래소 잔고 `0`을 확인할 수 있습니다. 화면의 초과출금 거절은 서명 전 사전 검사이며 체인 거절 시험을 대신하지 않습니다. 체인 권한·잘못된 서명·재전송 시험 결과는 기존 독립 QA/보안 증거에 포함돼 있습니다.

## 5. 선택 확인 — 조회 단절·재시작 보존

첫 입출금 시연이 끝나고 모든 TX가 확정된 다음 수행합니다. 브라우저 탭과 웹 서버는 유지합니다.

- API 터미널 C에서 Ctrl-C → 화면의 ‘확정 잔고 조회’: 조회 실패/단절 경고와 최신성 확인 불가를 확인합니다. 터미널 C에서 위 API 명령을 같은 hash·journal로 다시 실행하고 잔고를 재조회합니다. 자동 복구 알림이나 자동 잔고 갱신을 기대하지 않습니다.
- 노드 재시작 보존은 별도 터미널의 저장소 루트에서 아래 명령을 순서대로 실행하고, 마지막에 화면의 확정 잔고를 다시 조회합니다. `init`을 다시 실행하지 않습니다.

```sh
cd ~/cosmo-dex-s1
python3 ops/s1/devnet.py stop --home .runtime/wallet
python3 ops/s1/devnet.py start --home .runtime/wallet
python3 ops/s1/devnet.py health --home .runtime/wallet
```

기존 확정 잔고가 유지돼야 합니다. 1개/2개 검증인 장애와 자동화 시험은 [개발망 안내](../ops/s1/README.md)에 있습니다. 그 문서의 `tx --user` 및 fixture `integration.py`는 별도 합성 키 개발망용이므로, 여기서 만든 브라우저 공개키 개발망에 실행하지 마세요.

## 종료·재시작과 자주 만나는 문제

| 상황 | 조치 |
|---|---|
| 사용 종료 | 미확인 TX를 원래 hash로 먼저 조회합니다. 키를 폐기할 때 화면 reset을 누르고 각 서버 터미널에서 Ctrl-C로 종료합니다. |
| 탭·키는 유지, 서버만 재기동 | 같은 binary·home·hash·journal로 `serve`와 API를 다시 실행합니다. `init`은 하지 않습니다. |
| 탭을 새로고침/종료했음 | 키 복구는 불가능합니다. 새 탭에서 새 키를 만들고, 새 공개키 파일과 새 home(예: `.runtime/wallet-2`)으로 새 개발망을 준비합니다. 이전 서버는 먼저 종료하고, 새 manifest·journal·hash 경로를 모두 맞춥니다. 기존 DB는 보존합니다. |
| 화면 403 | URL을 정확히 `http://127.0.0.1:8080`으로 맞춥니다. |
| Go 버전 오류 | `go version`이 1.26.5인지 확인합니다. 이 빌드는 버전을 고정합니다. |
| `users.json` 오류 | 배열 전체인지, 정확히 공개키 2개인지 확인합니다. `python3 -m json.tool users.json`은 JSON 구문만 검사하며 공개키 내용 검증은 init이 합니다. |
| 이미 존재하는 home | 덮어쓰기·자동 reset 대신 이전 세션인지 확인합니다. 새 키는 다른 빈 home을 사용합니다. |
| 포트 사용 중 | 기존에 자신이 실행한 서버인지 확인하고 그 터미널에서 종료합니다. 무관한 프로세스를 강제 종료하지 않습니다. |
| 잔고가 안 보임 | 개발망 health, API 터미널 오류, 동일 genesis hash, 계정 전환 후 재조회 여부를 확인합니다. 노드 로그는 `python3 ops/s1/devnet.py log --home .runtime/wallet --node 0`입니다. |

## 아직 제공하지 않는 사용자 기능

주문장·매수/매도 주문·부분 체결·취소·매칭, 체결의 온체인 정산, 타인 주소 송금, 영속 지갑 백업/복구, 독립 비상 회수, 외부 코인/은행 입출금·법정화폐 상환은 현재 사용자 기능에 포함되지 않습니다. 실자산 서비스·지속 성능도 검증 범위 밖입니다. 원래 제품 통합시험 T01~T16은 전체 PASS 0/16이고 부분 실행 또는 NOT_RUN 상태를 유지합니다.

현재 main의 로컬 시연을 기준으로 안내합니다. 공유 Paperclip runtime이 최종 SHA로 갱신됐는지는 확인되지 않았습니다. 다음 기능 단계 후보 S2는 주문·매칭이며 이번 안내 요청으로 새 구현을 시작하지 않았습니다.


이 안내는 Paperclip 사용자 안내 revision `ed7d05e0-87d2-4b6e-842b-200952573457`을 저장소로 옮기고 코드와 대조했습니다. 원본과 역사적 실패는 [검증 기록](verification.md)에 연결합니다.
