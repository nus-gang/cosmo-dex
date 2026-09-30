# S1 지갑 연결 후보 — 실제 통합 미완료

NUS-22 승인 Plan의 두 사용자·DEVQUOTE 입출금 범위다. S0 파일을 바꾸지 않고
`web/s1`에 격리했다. 기반 Chain commit은
`ab3e8a8eebe47439a8102b30367396faa291e296`이다.

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
실제 D runtime 배포 주소 확인 전이다. HTTP 성공/CheckTx 성공으로 확정하지 않는다.
키 생성·reset 화면 자체 검증은 `node s1/browser.test.mjs`다.

## 계정 준비와 세션 수명

1. **두 시험 계정 생성**으로 브라우저 CSPRNG 기반 ML-DSA-65 키를 만든다.
2. 화면의 공개키 배열만 genesis 준비 담당자에게 제공한다. 두 키는
   canonical base64 raw1952이며 개인키/seed를 내보내는 기능은 없다.
3. NUS-27에서 제공할 공개키 genesis 입력으로 새로운 home과 개발망을 만든다.
   현재 기반 nusd init은 공개 fixture 키만 지원하므로 이 단계는 아직 차단되어 있다.
4. 실제 genesis 파일 SHA256을 화면에 입력해 고정한다. REST network의
   chain/genesis/denom/decimals와 등록 공개키·owner를 확인한 후 서명한다.
5. **같은 탭을 유지한다.** 새로고침/종료/pagehide/reset은 키 사용을 끝낸다.
   영속 저장·복구·seed import/export가 없으므로 기존 계정을 복구할 수 없다.
   JS 메모리의 완전 소거를 보장하지 않으며 보유 secret buffer는 명시적으로 0으로 덮는다.
6. 새 세션에는 새 공개키, 새 genesis 및 새 home을 준비한다. 기존 DB에 genesis를
   바꾸지 않는다. 미확인 TX가 남으면 화면 reset은 거절한다. 강제 탭 종료 시에는
   공개 TX hash를 보존해 체인에서 확인해야 한다. 기존 개발망을 재사용해 복구됐다고 표시하지 않는다.

## 입출금 시연 절차 (실행 전)

각 사용자에서 잔고 조회 → 100 예치 → TX 결과 조회에서 확정 → 40 출금 → 확정 조회 →
거래소 잔고 60 확인 → 60 초과 출금 거절을 확인한다. 표시 decimals=6이므로
100=100000000 atoms, 40=40000000 atoms다. 입력에 부동소수 연산을 쓰지 않는다.
계정별 요청은 결과가 불명확한 동안 잠기며 자동 새 sequence/ID 재서명은 없다.
조회는 사용자가 명시적으로 실행한다. 인덱스 유실/지연 시 UNKNOWN을 유지한다.
최초 성공 receipt를 이용한 추가 복구 UX는 실제 D 연결 시 검증할 항목이다.

가스는 1000 DEVGAS atoms/gas limit 500000을 명시적으로 보여준다. 확정 결과는
동일 TX hash, 양수 inclusion height, code와 state의 일치로만 갱신한다.
최소 화면은 주소·bank/exchange/gas·epoch/sequence·관측 높이·TX 내역을 제공한다.
출금 수취인은 owner 고정이다. 사전 초과출금 거절은 chain의 권한/잔고 거절 시험을 대신하지 않는다.

## 검증 범위

`direct-vectors.json`은 CTO `protocol/s1/vectors/direct.json`의 공개키·Go 서명·바이트를
복사하고 공개 seed 필드도 제거한 시험 데이터다. 런타임 번들에서 import하지 않는다.
Go golden 3건 message/body/auth/SignDoc 일치와 Go 서명 검증, 정수 경계,
키 폐기, 중복 클릭/응답 유실/404 잠금, 잘못된 결과 거절, 초과출금 사전 거절을 확인했다.
별도 S1 타입 검사와 esbuild 성공. 실제 SDK가 TS 생성 TX를 수락하는 시험,
4검증인 C/D 연동·100→40→60 시연·독립 보안 심사·PR/main 인수는 **NOT_RUN**이다.
Helix/WS/거래 화면/백업은 추가하지 않았다. 기존 lockfile 의존성을 재사용한다.
