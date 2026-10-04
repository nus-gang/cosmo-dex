# S2 시험 지갑

[NUS-40](/NUS/issues/NUS-40)의 지갑 구현. 승인된 S2-D
`9b38bdae33e964ea70a8fd3d8edb00b887d48933`와 DIRECT 조회
[NUS-48](/NUS/issues/NUS-48) `a34ecb0f1ca2f26e14599d19d5e2be1b4e106bd9`를 사용한다.
착수·재개 시 원격 main은 `24029b811e5ec798bbe57f769de3d3f254c90ab7`이었다.
공통 protocol/S0/S1과 의존성 pin/lock은 변경하지 않았다.

## 실행

```sh
cd web
npm ci --ignore-scripts
./node_modules/.bin/tsc --noEmit -p s2/tsconfig.json
node --experimental-strip-types --test s2/*.test.ts
node s2/build.mjs
node s2/serve.mjs
```

화면은 `http://127.0.0.1:5173`, API는 `http://127.0.0.1:8788`로 고정한다.
관리 runtime이 있는 환경에서는 해당 서비스 제어를 사용한다.
화면에서 두 시험 키를 생성하고 공개키 배열을 nusd `init --network s2 --user-public-keys`
입력으로 사용해 **새 home/genesis**를 만든다. 기존 S1 데이터를 재사용하지 않는다.
[API bootstrap/실행 안내](../../settlement/s2/README.md)에 따라 실제 genesis를 검증한
후 화면에 그 bytes의 SHA256을 고정한다. 탭 새로고침은 키를 잃으므로 하지 않는다.

각 계정에서 DEVBASE/DEVQUOTE를 선택해 예치하고 TX 결과를 조회한다. 다음으로
서명 로그인 후 매도자 2 BASE/10 QUOTE GTC, 매수자 1 BASE/10 QUOTE GTC,
매도자의 잔량 취소, 별도 maker 0.5 BASE와 buyer 1 BASE/10.001 QUOTE IOC를 실행한다.
두 자산은 소수점 6자리, 주문 lot/tick은 소수점 3자리까지 문자열 정수로 계산한다.
수수료는 1000 DEVGAS atoms, gas limit 500000, TX/주문 만료는 관측 높이+100이다.
금액 입력의 자산 단위와 표의 atoms 단위(1 자산=1,000,000 atoms)를 구분한다.

일반 출금은 서명 로그인 후 freeze→미체결 취소→D/P 검사를 거친다.
`UNSETTLED_HOLD`는 ‘정산 미구현/미정산 보류’로 표시하며 TX를 만들지 않는다.
준비 해제 버튼은 엔진 동결을 해제한다. 별도 직접 출금은 엔진 세션/OPEN을 요구하지
않고 확정 C에서 출금한다. owner epoch 변경은 양측 잠정 체결을 정정할 수 있다.
직접 TX와 확정 계정 조회는 API의 trusted loopback RPC 연결이 필요하다.

## 상태·키 경계

키·토큰은 탭 메모리에만 유지한다. runtime 키 import/export·복구·서버 서명은 없다.
C/R/D/P/A를 따로 표시하고 P를 A에 더하지 않는다. 체결은 잠정/정정이며 온체인 정산이 아니다.
LOCAL_ACCEPTED는 LOCAL_FSYNC/replicated=false다. REST 1초 polling을 사용한다.
서버 관측 age + 전체 조회/페이지 전달 시간 + 수신 후 경과가 5초를 넘으면 주문 접수를 닫는다.
경과는 벽시계와 단조 시계 중 큰 값으로 계산한다. 시간 역행·단절 시에는 신선한 조회를
다시 받아야 재개하며, 로그인 만료도 입력을 닫는다. 관측 높이·지연·사유를 표시한다.
계정 전환 시 개인 화면과 세션을 폐기하고 지연 응답을 버린다. seq/revision이 역행하거나
같은 snapshot 내 pagination이 충돌하면 과거/부분 내역으로 화면을 바꾸지 않는다.
UNKNOWN 주문은 receipt 확인 후 같은 서명 원문/ID로만 재시도한다.
UNKNOWN TX는 새 서명을 잠그고 같은 hash의 확정 결과를 조회한다. 404나 timeout은
실패 확정이 아니다. 현재 화면은 불확실 TX의 자동 재전송을 제공하지 않는다.

## 실제 브라우저 인수

`browser.test.mjs`는 제품 bundle을 클릭해 사용하는 유한 시험이다. managed execution
workspace가 없는 환경에서만 자체 자식 서비스들을 띄우며 finally에서 모두 종료한다.
체인 개인키 파일/home은 run scratch에만 두고 artifact로 게시하지 않는다.
브라우저 생성 비밀키는 page.evaluate나 파일로 추출하지 않는다. 공개키·서명 TX·원장·
RPC 근거만 보존하며 세션 bearer는 캡처하지 않는다.

```sh
# 저장소 루트, 기존 pin/lock으로 빌드한 바이너리와 Chromium을 지정한다.
S2_TEST_CHAIN=/path/to/approved/nusd S2_ENGINE_BINARY=/path/to/exchange-s2 \
  S2_TEST_SCRATCH=/path/to/private/scratch CHROME_BIN=/path/to/chrome \
  node web/s2/browser.test.mjs /path/to/new/evidence
```

두 사용자×두 자산의 실제 4 예치 TX, GTC/부분 체결/취소/제한 IOC,
C/R/D/P/A 정확한 정수, 보류 출금, 직접 출금·양측 정정, 엔진/API 재시작 후 fill ID 보존,
D/P=0 정상 출금, 지연된 계정 조회와 계정 전환, 실제 RPC 중단을 검증한다.
단위 시험은 역순·동일 seq 충돌·신선도·UNKNOWN·계정 전환과 공통 서명 벡터를 검증한다.
CI `S2 Wallet`은 Linux에서 동일 제품 브라우저 시험과 S0/S1 회귀를 실행한다.

이 증거는 단일 검증인 개발망이다. 4검증인 통합, 독립 Security/QA와 main 인수는
후속 전문 단계에서 판정한다. S3 정산·분산 내구성·독립 회수·WS 전체·처리량 목표의
PASS 근거로 사용하지 않는다.
