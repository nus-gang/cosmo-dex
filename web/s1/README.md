# S1 지갑·REST 통합 후보

> 현재 제품 기준 main `32781aa97d62ec747e7a25c10fdb8b58030d79f2`에서 S1 인수가 완료됐다. 처음 실행하면 [사용자 시작 안내](../../docs/quickstart.md)를 따른다. 아래 후보 SHA·검토 대기 표현은 개발 당시 기록이며 현재 인수 근거는 [검증 기록](../../docs/verification.md)에 연결한다.

NUS-22 승인 Plan의 두 사용자·DEVQUOTE 입출금 범위다. S0 파일을 바꾸지 않고
`web/s1`에 격리했다. C/D 승인 후보 `2f1e498d6a591d63fa74c7285b9d396173d1476c`를 인수했다.
공개키 genesis 입력과 실제 REST 계약 수정을 포함한다.

## 실행

Node >=22.18.0, Chrome, 기존 package-lock.json이 필요하다. web 디렉터리에서:

```sh
npm ci --ignore-scripts
./node_modules/.bin/tsc -p s1/tsconfig.json
node --experimental-strip-types --test s1/direct.test.ts
node s1/build.mjs
S1_API=http://127.0.0.1:8787 node s1/serve.mjs
```

`http://127.0.0.1:8080`을 연다. proxy는 loopback HTTP API만 허용한다.
API는 S1-D의 `/s1/*` 계약을 제공해야 한다. 포트 8787은 실행 예시이며
선택한 로컬 REST 포트와 일치시킨다. HTTP 성공/CheckTx 성공으로 확정하지 않는다.
키 생성·reset 화면 자체 검증은 `node s1/browser.test.mjs`다.

## 계정 준비와 세션 수명

1. **두 시험 계정 생성**으로 브라우저 CSPRNG 기반 ML-DSA-65 키를 만든다.
2. 화면의 공개키 배열만 genesis 준비 담당자에게 제공한다. 두 키는
   canonical base64 raw1952이며 개인키/seed를 내보내는 기능은 없다.
3. `ops/s1/devnet.py init --user-public-keys <공개키 JSON>`으로 새로운 home과
   4검증인 개발망을 만든다. 공개키 genesis를 지원하는 현재 Chain binary를 사용한다.
4. 실제 genesis 파일 SHA256을 화면에 입력해 고정한다. REST network의
   chain/genesis/denom/decimals와 등록 공개키·owner를 확인한 후 서명한다.
5. **같은 탭을 유지한다.** 새로고침/종료/pagehide/reset은 키 사용을 끝낸다.
   영속 저장·복구·seed import/export가 없으므로 기존 계정을 복구할 수 없다.
   JS 메모리의 완전 소거를 보장하지 않으며 보유 secret buffer는 명시적으로 0으로 덮는다.
6. 새 세션에는 새 공개키, 새 genesis 및 새 home을 준비한다. 기존 DB에 genesis를
   바꾸지 않는다. 미확인 TX가 남으면 화면 reset은 거절한다. 강제 탭 종료 시에는
   공개 TX hash를 보존해 체인에서 확인해야 한다. 기존 개발망을 재사용해 복구됐다고 표시하지 않는다.

## 입출금 시연 절차

각 사용자에서 잔고 조회 → 100 예치 → TX 결과 조회에서 확정 → 40 출금 → 확정 조회 →
거래소 잔고 60 확인 → 60 초과 출금 거절을 확인한다. 표시 decimals=6이므로
100=100000000 atoms, 40=40000000 atoms다. 입력에 부동소수 연산을 쓰지 않는다.
계정별 요청은 결과가 불명확한 동안 잠기며 자동 새 sequence/ID 재서명은 없다.
조회는 사용자가 명시적으로 실행한다. 인덱스 유실/지연 시 UNKNOWN을 유지한다.
receipt 조회는 REST에서 제공하며 이 화면은 원래 TX hash로 결과를 확인한다.

가스는 1000 DEVGAS atoms/gas limit 500000을 명시적으로 보여준다. 확정 결과는
동일 TX hash, 양수 inclusion height, code와 state의 일치로만 갱신한다.
최소 화면은 주소·bank/exchange/gas·epoch/sequence·관측 높이·TX 내역을 제공한다.
출금 수취인은 owner 고정이다. 사전 초과출금 거절은 chain의 권한/잔고 거절 시험을 대신하지 않는다.

## 검증 범위

`direct-vectors.json`은 CTO `protocol/s1/vectors/direct.json`의 공개키·Go 서명·바이트를
복사하고 공개 seed 필드도 제거한 시험 데이터다. 런타임 번들에서 import하지 않는다.
Go golden 3건 message/body/auth/SignDoc 일치와 Go 서명 검증, 정수 경계,
키 폐기, 중복 클릭/응답 유실/404 잠금, 잘못된 결과 거절, 초과출금 사전 거절을 확인했다.
별도 S1 타입 검사와 esbuild 성공. NUS-29에서 기본 fetch 바인딩과
공개키 type URL 수정 후보로 실제 SDK의 TS 생성 TX 수락, 새 4검증인 C/D 연동,
두 사용자 100→40→60, 응답 유실·UNKNOWN 잠금·동일 bytes 재전송을 검증했다.
이 통합 후보는 해당 수정과 Wallet 8d3892c를 포함한다. 검증 원본·화면은
Paperclip NUS-29 첨부와 NUS-21 통합 결과 문서에 있다. 독립 보안 심사·main 인수는
NUS-21 후보 심사와 별개로 NUS-22 제품 심사 및 main 인수가 필요하다.
Helix/WS/거래 화면/백업은 추가하지 않았다. 기존 lockfile 의존성을 재사용한다.

## 전체 로컬 실행 안내

저장소 root에서 Go 1.26.5로 `cd chain/app && sh scripts/build.sh`를 실행한다.
위 web 설치/build/serve 명령으로 화면을 먼저 열고 **두 시험 계정 생성**을 누른다.
화면의 공개키 JSON 배열만 저장소 밖 `users.json` 파일로 저장한다. 같은 탭을 유지한다.
별도 터미널의 저장소 root에서 다음을 실행한다(Paperclip 토큰 불필요).

```sh
python3 ops/s1/devnet.py init --home .runtime/wallet --user-public-keys users.json
python3 ops/s1/devnet.py serve --home .runtime/wallet
```

다른 터미널에서 `.runtime/wallet/manifest.json`의 `genesis_sha256`을 읽는다.
그 값을 아래 `<HASH>`와 화면 genesis 입력에 동일하게 넣는다.

```sh
python3 settlement/s1/server.py --rpc http://127.0.0.1:28657 \
  --genesis-hash <HASH> --journal .runtime/wallet/txs.sqlite \
  --port 8787 --origin http://127.0.0.1:8080
```

각 계정에서 위 시연을 실행한다. 60.000001 출금은 `INSUFFICIENT_BALANCE`로
서명·POST 전에 거절된다. 이는 UI 사전검사이며 체인 자체 거절 검증을 대신하지 않는다.
사용을 마치면 모든 TX를 조회한 뒤 reset하고 각 터미널에서 Ctrl-C로 종료한다.
다음 세션은 새로운 키와 **다른 빈 home**으로 init한다. 기존 home을 덮어쓰지 않는다.
브라우저 키를 닫은 뒤 기존 계정에 다시 접근할 수 있는 백업/복구 기능은 없다.
공개키 배열은 키 복구 파일이 아니다.

Paperclip의 지속 서비스는 관리 runtime 설정·start를 사용한다. 아래 자동시험은
유한한 자식 프로세스이며 공유 SRE 개발망을 변경하지 않고 finally에서 모두 종료한다.

## 재현 가능한 브라우저 통합 시험

```sh
# web에서 실행. 체인 binary는 위 명령으로 먼저 빌드한다.
S1_TEST_SCRATCH=<새_전용_디렉터리> node s1/integration.mjs ../.evidence/wallet
```

Paperclip에서는 주입된 `PAPERCLIP_RUN_SCRATCH_DIR`를 사용한다. `CHROME_BIN`으로
Chrome 경로를 지정할 수 있다(macOS 기본 Chrome 경로 사용). `S1_BINARY`를 지정하면
그 바이너리의 실행 SHA/hash를 manifest에 기록한다. 출력 디렉터리는 공개 증거만
포함하며 home/DB/검증인 키는 scratch에 남는다. 이를 첨부하지 않는다.

시험 전용 포트는 Wallet 18081, REST 18787, 검증인 P2P/RPC 30656/30657부터
10씩 증가하는 네 쌍이다. 포트가 사용 중이면 먼저 해당 시험 프로세스가 종료됐는지
확인한다. 다른 서비스의 포트를 재사용하거나 프로세스를 강제 종료하지 않는다.

검증: 두 브라우저 무작위 키, 공개키 genesis, 실제 4검증인 합의·SDK DIRECT 수락,
각 100→40→60, 응답 유실 후 원래 hash 조회, UNKNOWN 중 새 서명 잠금,
중복 클릭 1 POST, 같은 bytes 재전송 불변, 두 계정 초과출금 사전 거절,
공개 SignDoc 서명·receipt 대사, 390px 표시, 완료 후 reset.
`result.json`, 공개키, 화면 PNG를 증거로 저장한다. 강제 응답 유실·UNKNOWN은
브라우저 route에서 주입한 시험 조건이다. 실제 네트워크 자연 장애로 보고하지 않는다.

S1 Wallet browser integration CI는 항상 실행되며 자체 새 4검증인으로 동일 시험을
수행한다. 로컬 PASS와 원격 head CI 결과는 별도로 기록한다.
