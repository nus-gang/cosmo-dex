# S2 두 자산·주문 시작 안내

[문서 목차](README.md) · [S1 재현](quickstart.md) · [S2 운영 상세](../ops/s2/README.md) · [검증 기록](verification.md)

이 안내는 **검증된 main** `aad654bcf6760bc9af162b681a5996487ffc715e`의 S2 로컬 PoC를 재현한다. 한 호스트의 4검증인에서 DEVBASE/DEVQUOTE를 실제 예치하고 서명 주문·부분 체결·취소·가격 제한 IOC를 확인한다. 체결은 **잠정**이며 온체인 정산과 수취 자산 출금은 제공하지 않는다. S2 main 인수는 완료됐으며 [독립 QA·최종 승인·알려진 관측](verification.md#s2-main-인수)을 함께 확인한다.

키는 생성한 브라우저 탭 메모리에만 있다. **새로고침·탭 종료 후 복구할 수 없으며 공개키 JSON은 지갑 백업이 아니다.** 기존 S1 DB를 업그레이드하지 않는다. 별도 S2 home·genesis·journal을 만들고 기존 원장을 보존한다.

## 준비와 빌드

개인 개발 PC의 macOS/Linux, Git 저장소 접근, Go **1.26.5**, Rust **1.92.0**, Node **24.21.0**(루트 `.node-version`), npm, Python **3.10 이상**, Chrome/Chromium을 준비한다. lock을 변경하지 않는다. 의존성 다운로드 연결이 필요하며 최소 RAM/디스크·지속 처리량은 미측정이다. DEVGAS는 가스 전용 합성 자산이다.

빈 loopback 포트는 웹 5173, API 8788, P2P/RPC 30556/30557·30566/30567·30576/30577·30586/30587이다. 기존 서비스를 종료시키거나 S1의 28657 계열 home을 재사용하지 않는다.

새 폴더에서 다음을 실행한다. 폴더가 이미 있으면 삭제하지 말고 다른 이름을 사용한다.

```sh
git clone https://github.com/nus-gang/cosmo-dex.git cosmo-dex-s2
cd cosmo-dex-s2
git fetch origin aad654bcf6760bc9af162b681a5996487ffc715e
git checkout --detach aad654bcf6760bc9af162b681a5996487ffc715e
go version
rustc +1.92.0 --version
node --version
python3 --version
(cd chain/app && sh scripts/build.sh)
cargo +1.92.0 build --locked --manifest-path exchange/Cargo.toml --bin exchange-s2
(cd web && npm ci --ignore-scripts && node s2/build.mjs)
```

기대 산출물은 `chain/app/bin/nusd`, `exchange/target/debug/exchange-s2`, `web/dist/s2/`다. 아래 수동 절차는 **사용자 PC의 foreground 터미널**용이다. Paperclip 관리 환경에서는 임시 웹과 통합 서비스 모두 승인된 runtime 제어를 사용한다. 현재 상시 preview는 미등록·미검증이며 이 문서는 서비스 등록 승인이나 기존 runtime 변경 지시가 아니다.

## 1. 웹에서 두 키를 만들고 새 home 준비

터미널 A에서 저장소 루트 기준으로 실행한다.

```sh
node web/s2/serve.mjs
```

Chrome에서 정확히 `http://127.0.0.1:5173`을 열고 **두 시험 계정 생성**을 누른다. **새 genesis 등록용 공개키**의 JSON 배열 전체를 저장소 루트 `users.json`에 저장한다. 두 공개키 문자열만 담고 코드펜스는 넣지 않는다. API 기동 전 연결 오류는 이 단계에서 정상이다.

**탭을 그대로 유지**하고 터미널 B에서 실행한다.

```sh
python3 -m json.tool users.json
python3 ops/s2/runtime.py init --home .runtime/s2 --user-public-keys users.json
```

`init`은 정확히 한 번 실행하며 기존 home이면 거절한다. `--user-public-keys`를 생략하면 fixture 계정이 생성되어 브라우저 키와 일치하지 않는다. 출력의 `genesis_hash`를 보관한다. 사용자당 각 자산 지갑 초기값은 `1000000000000` atoms, 가스 `1000000000` atoms, 거래소 확정 C는 0이다.

터미널 A에서 Ctrl-C로 **임시 웹 서버만** 종료한다. 탭은 닫거나 새로고침하지 않는다. 같은 터미널에서 통합 foreground 명령을 실행한다. 웹 포트가 해제돼야 한다.

```sh
python3 ops/s2/runtime.py serve --home .runtime/s2
```

통합 runtime이 네 검증인→bootstrap→API/엔진→웹을 시작한다. Paperclip에서는 이 명령을 등록한 관리 서비스의 start를 사용한다. 터미널 B에서 준비 상태와 정확한 hash를 확인한다.

```sh
python3 ops/s2/runtime.py health --home .runtime/s2
python3 -c 'import json; print(json.load(open(".runtime/s2/runtime.json"))["chain_genesis"])'
```

노드 4개 모두 높이 >0·`catching_up: false`, API `mode: OPEN`, web `200`이면 준비됐다. 초기 catch-up 중이면 잠시 뒤 health를 다시 실행한다. 출력 hash를 열린 탭의 **실제 genesis SHA256**에 넣고 **genesis 고정**을 누른다. 프로토콜 fixture hash를 사용하지 않는다.

## 2. 실제 예치 후 지정가·부분 체결·취소

계정 전환 때마다 **서명 로그인 / 재연결**을 누른다. 세션 만료(300초)·API 재시작 뒤에도 재로그인한다. 예치 전 **확정 체인 계정 조회**, 제출 후 **TX 결과 조회**로 해당 hash의 확정을 확인한다. `SUBMISSION_UNKNOWN`은 실패도 확정 성공도 아니다.

| 순서 | 화면 조작 | 확인할 결과 |
|---|---|---|
| 1 | 사용자 1(A), 자산 DEVBASE, 금액 `10` → 예치 서명·제출 → TX 결과 조회 | 확정 C BASE `10000000` atoms |
| 2 | 사용자 2(B), DEVQUOTE, `100` → 예치·확정 확인 | 확정 C QUOTE `100000000` atoms |
| 3 | A 로그인, SELL/GTC, 수량 `2`, 한도가 `10` → 위 조건으로 서명·제출 | 호가 잔량 2 BASE; A BASE R `2000000` |
| 4 | B 로그인, BUY/GTC, 수량 `1`, 한도가 `10` → 제출 | 잠정 1 BASE 체결, A 잔량 1 BASE |
| 5 | A 로그인, 내 주문에서 잔량 취소 | 미체결 R만 해제, D/P 유지 |

4번 직후 A의 BASE는 C=10000000/R=1000000/D=1000000/A=8000000, QUOTE P=10000000이다. B의 QUOTE는 C=100000000/R=0/D=10000000/A=90000000, BASE P=1000000이다. 5번 후 A의 BASE R=0/A=9000000이다. 체결 후에도 체인 C는 A BASE 10, B QUOTE 100으로 유지된다.

공개 호가는 `ticks / lots`로 표시한다. 가격 10은 `10000` ticks, 수량 2/1/0.5는 `2000`/`1000`/`500` lots다.

입력은 **자산 단위**, 원장 표는 **atoms**다. 두 자산 `decimals=6`, 1 자산=1000000 atoms. 수량 lot=0.001 BASE, 가격 tick=0.001 QUOTE/BASE이며 각각 소수점 3자리까지 입력한다. 기본 거래 수수료 0bps와 TX 수수료 **1000 DEVGAS atoms**(0.001 DEVGAS), gas limit **500000**은 서로 다르다. 주문/TX 만료는 관측 확정 높이+100이다.

## 3. 별도 IOC와 출금 제한 확인

새 주문으로 A SELL/GTC `0.5` BASE / 가격 `10`, B BUY/IOC `1` BASE / 한도가 `10.001`을 제출한다. 가격 10에서 0.5만 잠정 체결되고 IOC 잔량 0.5는 즉시 취소된다. 이전 체결 기록은 유지한다.

0bps에서 누적 A BASE D=1500000/QUOTE P=15000000, B BASE P=1500000/QUOTE D=15000500 atoms다. B의 D는 실제 가격 개선분을 즉시 돌려주지 않고 한도가 기준 최악 입력액을 유지한다. R=0이며 B QUOTE A=84999500 atoms다. 이 값은 앞 시나리오에 이어 실행했을 때의 값이다.

**일반 출금 준비·서명·제출**은 신규 매칭 동결→미체결 취소→D/P 검사 순서다. 이 상태에서는 `UNSETTLED_HOLD`(정산 미구현/미정산 보류)로 중단하고 출금 TX를 만들지 않는다. P는 재주문·출금 재원이 아니다.

출금 준비를 해제한 뒤 재시작 시연을 계속하려면 다음 순서를 따른다.

1. 준비 요청 당시의 확정 관측 높이를 기록한다. 해제 직전에 준비를 다시 누르면 준비 높이도 다시 설정되므로 누르지 않는다.
2. 터미널 B에서 다음 명령으로 **전역 runtime health**를 확인한다. 같은 S2 home을 사용하며, 별도 home 경로를 지정했다면 그 경로로 바꾼다.

   ```sh
   python3 ops/s2/runtime.py health --home .runtime/s2
   ```

   출력 JSON의 **`api.mode="OPEN"`**, **`api.observation.fresh=true`**, **`api.observation.observed_height`가 준비 당시 높이보다 큼**을 모두 확인한다(높이 문자열은 정수로 비교한다). 조건을 만족하지 않으면 health를 다시 확인하고 전역 RPC/신선도 장애를 먼저 복구한다. 같은 높이에서 해제를 누르면 `STALE`로 거절되며 동결은 유지된다. 몇 초 기다렸다는 사실만으로 해제를 보장하지 않는다.

   전역 health가 OPEN/fresh여도 준비한 계정의 개인 화면은 **접수 닫힘 · 마지막 관측값 · WITHDRAW_FROZEN**일 수 있다. 이는 해제 전 정상적인 계정 동결 표시이며, 개인 화면의 **접수 가능 · OK를 기다리지 않는다**. 개인 화면과 전역 health의 관측 시점·높이도 다를 수 있으므로 위 전역 health 출력으로 조건을 확인한다.
3. 조건을 확인한 뒤 **출금 준비 해제**를 누른다. 이미 `STALE`을 받았다면 버튼을 **다시 눌러 명시적으로 재시도**한다. polling으로 높이가 갱신돼도 자동으로 해제되지는 않는다.
4. **출금 준비 동결 해제** 완료 표시를 확인한다. 해제는 D/P 정산·취소 주문 복구·출금 권한 부여가 아니다. D/P와 취소 기록은 유지되고 추가 출금 TX는 없어야 한다. 아래 절차에서 같은 탭/키와 같은 home을 유지해 재시작한다.

전역 OPEN과 개인 WITHDRAW_FROZEN의 구분은 [개인 조회 구현](../exchange/src/s2/service.rs)의 `ledger_view`, health 출력의 `api` 필드는 [runtime 명령](../ops/s2/runtime.py)을 따른다. 높이·신선도 조건과 원장 검증은 [해제 구현](../exchange/src/s2/sequencer.rs)의 `abort_withdraw`, 같은 높이 거절→다음 높이 성공·취소 주문 미복구는 [회귀시험](../exchange/tests/s2_sequencer.rs)의 `withdraw_cancels_only_open_reserve_and_retains_pending_hold`를 따른다.

D/P=0인 별도 새 시나리오에서는 일반 출금이 가능하다. **직접 출금**은 엔진 승인 없이 확정 C에서 서명하는 별도 경로이며 잠정 P의 출금이 아니다. 실제 확정 출금으로 owner epoch가 바뀌면 구 주문과 상대방·후속 의존 체결이 동결/정정된다. 이 정정은 온체인 체결 정산이 아니다. 기본 시연 수치를 보존하려면 직접 출금 실험은 별도 새 home에서 수행한다. [자동 브라우저 시험](../web/s2/README.md)은 직접 출금·양측 정정 후 D/P=0 일반 출금을 별도로 검증한다.

## 종료·같은 home 재시작·문제 해결

미확인 TX는 원래 hash, UNKNOWN 주문은 원래 명령 receipt로 먼저 확인한다. 주문 재시도는 **동일 서명 원문/ID**를 사용한다. 404·timeout은 실패 확정이 아니며 새 ID나 새 TX를 자동 생성하지 않는다.

사용자 PC에서는 통합 runtime 터미널에서 Ctrl-C 후 종료 결과를 확인한다. Paperclip에서는 관리 stop→stopped 확인을 사용한다. 재시작 시험 중에는 브라우저 탭을 유지한다. 아래는 노드 상태와 보존 파일 확인용이다.

```sh
python3 ops/s1/devnet.py status --home .runtime/s2/chain
python3 ops/s1/devnet.py log --home .runtime/s2/chain --node 0
```

소유 프로세스·포트·lock 해제를 확인한 뒤 같은 `runtime.py serve --home .runtime/s2`(관리 환경은 start)로 재기동한다. **init을 다시 실행하지 않는다.** 같은 binary/build pin·DB·journal·genesis를 사용하고 health OPEN 후 재로그인한다. 주문/fill ID·취소·C/R/D/P를 이전 기록과 대조한다. 서버 기록 복원은 탭 키 복원이 아니다.

| 상황 | 다음 행동 |
|---|---|
| 준비 해제에서 같은 높이 `STALE` | `python3 ops/s2/runtime.py health --home .runtime/s2` 출력에서 `api.mode=OPEN`·`api.observation.fresh=true`·준비 높이보다 높은 `api.observation.observed_height`를 확인한 뒤 **출금 준비 해제**를 다시 누르고 **출금 준비 동결 해제** 표시를 확인한다. 자동 해제나 대기 시간만으로 성공을 가정하지 않는다. |
| 개인 화면 `WITHDRAW_FROZEN` | 출금 준비 후 해제 전 정상적인 계정 동결 표시다. 개인 화면 접수 가능을 기다리지 말고 §3의 전역 health·높이 조건을 확인해 명시적으로 해제한다. |
| 실제 전역 RPC 장애·지연·STALE | 전역 health의 관측 높이·사유·fresh와 RPC를 확인해 복구한다. REST polling은 1초, 총 신선도 5초 초과·높이 역행·catch-up 미완료 때 신규 주문을 닫는다. 높이 증가만으로 복구를 판단하지 않는다. |
| UNKNOWN 주문·미확인 TX | 원래 명령 receipt·TX hash를 조회한다. 404·timeout을 실패 확정으로 보지 않으며, 주문 재시도는 동일 서명 원문/ID를 쓴다. 새 ID·새 TX로 대체하지 않는다. |
| 403 또는 웹이 안 열림 | `http://127.0.0.1:5173`을 사용한다. init 전 임시 웹이 남아 있으면 그 소유 터미널에서 종료한다. |
| 이미 존재하는 home·두 번째 writer | 기존 실행과 home 소유권을 확인한다. lock 삭제로 우회하지 않는다. |
| build pin/hash 불일치 | 초기화 당시 후보와 산출물을 대조하고 운영 담당자에게 검토받는다. 자동 pin 갱신·빈 journal 교체를 하지 않는다. |
| journal 손상·불확실한 완료 | 실행을 멈추고 원본 DB/journal·오류 증거를 보존해 Exchange/SRE에 전달한다. 삭제·truncate로 성공 처리하지 않는다. |
| 강제 종료·정리 오류 | 비0 종료를 정상 종료로 보지 않는다. SRE가 소유 PID/포트/lock/제어 소켓 해제를 확인한 뒤 재시작한다. |
| 탭 새로고침·종료 | 이전 키 복구 불가. 기존 원장을 보존하고 새 탭·새 공개키·별도 새 home을 준비한다. 공개키 파일로 이전 계정을 복구할 수 없다. |

일반 로그는 전체 최대 4000MiB(검증인 2000MiB + wrapper 2000MiB)다. DB·WAL·journal/outbox·키·genesis·원시 증거는 로그 정리 대상이 아니다. 자세한 종료 시간·로그 오류 처리는 [운영 안내](../ops/s2/README.md)를 따른다. 기존 데이터를 지우는 reset/자동 migration은 제공하지 않는다.

## 자동 재현과 검증 범위

독립 QA의 수동 안내 재현과 별도로, 빌드 후 다음 유한 시험은 새 scratch home에서 실제 브라우저 시연을 실행하고 자식을 종료한다. 관리 workspace에서는 승인된 실행 경로가 필요하다. 기존 서비스와 포트가 충돌하면 중지하고 담당자에게 격리 환경을 요청한다.

```sh
(cd web && npx playwright-core install chromium)
mkdir -p .evidence/s2-docs/scratch
S2_FOUR_VALIDATORS=1 S2_TEST_CHAIN="$PWD/chain/app/bin/nusd" \
  S2_ENGINE_BINARY="$PWD/exchange/target/debug/exchange-s2" \
  S2_TEST_SCRATCH="$PWD/.evidence/s2-docs/scratch" \
  node web/s2/browser.test.mjs .evidence/s2-docs/browser
python3 ops/s2/manifest.py --output .evidence/s2-docs
```

Linux의 Chromium 시스템 라이브러리가 없으면 [CI](../.github/workflows/s2-integration.yml)의 `npx playwright-core install --with-deps chromium` 환경 준비가 필요하다. 성공 시 browser의 `result.json`과 manifest·공개 증거를 확인한다. **scratch는 개인키/node home을 포함할 수 있어 업로드하지 않는다.** 공개 증거만 게시하며 원본을 임의 삭제하지 않는다.

`LOCAL_ACCEPTED`는 `durability=LOCAL_FSYNC`, `replicated=false`다. 단일 호스트 fsync·재시작 재생을 분산 durable ACK·AZ 소실 복구로 해석하지 않는다. S3 정산·영속 키 복구·독립 비상 회수·외부 자산 게이트웨이·실자산 운영·WS 전체는 미제공이다. 기존 T01~T16 full PASS **0/16**, 부분/NOT_RUN 이력과 공유 runtime 최종 SHA 미확인 경계를 유지한다.
