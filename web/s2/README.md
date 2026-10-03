# S2 Wallet 작업 체크포인트

[NUS-40](/NUS/issues/NUS-40), 승인된 S2-D head
`9b38bdae33e964ea70a8fd3d8edb00b887d48933` 위의 Wallet 전용 브랜치.
원격 main 착수 조회: `24029b811e5ec798bbe57f769de3d3f254c90ab7`.

현재 주문/취소 서명, WalletChallenge 검증, REST 세션·조회·UNKNOWN 동일 원문 재시도,
sequence/revision 및 계정 전환 보호, C/R/D/P/A와 호가·주문·잠정/정정 화면 초안을 구현했다.
키·토큰은 탭 메모리에만 유지한다. runtime 키 import/export·복구·서버 서명은 없다.
S0/S1 소스와 기존 의존성 pin/lock, 공통 protocol은 변경하지 않았다.

**아직 전체 구현 완료나 브라우저 인수 PASS가 아니다.**
[NUS-48](/NUS/issues/NUS-48)에서 Settlement가 DIRECT 서명용 확정 계정 조회를 보완한다.
현재 `/s2/me`에는 account_number/sequence/등록키/bank/GAS가 없어서 실제 새 브라우저 키로
예치·출금할 수 없다. Wallet이 합성 값으로 대체하거나 RPC를 우회 연결하지 않는다.
보완 승인 후 기존 S1 DIRECT codec을 두 자산으로 연결하고 일반 출금 prepare/abort와
독립 직접 TX 경로를 완성한다. 실제 API 브라우저 두 계정 예치→부분 체결→취소/IOC→
재시작·epoch 정정·단절, 키 비노출 네트워크 캡처, CI/CTO→Security 인수는 남아 있다.

검증 명령 (Node 24.21.0, 기존 npm lock):

```sh
cd web
npm ci --ignore-scripts
./node_modules/.bin/tsc --noEmit -p s2/tsconfig.json
node --experimental-strip-types --test s2/s2.test.ts
node s2/build.mjs
npm test
node --experimental-strip-types --test s1/direct.test.ts
```

Paperclip 관리 runtime에서 `cd web && node s2/serve.mjs`로 정적 화면을 제공한다.
승인된 API는 `http://127.0.0.1:8788`, 화면은 `http://127.0.0.1:5173`다.
임의 origin/API 주소를 받지 않으며 서버 프로세스 기동은 별도 관리 runtime으로 수행한다.
현재 체크포인트에서 live preview를 기동하거나 runtime 서비스를 등록하지 않았다.

화면에서 두 시험 키를 만든 후 공개키 배열로 **새 S2 genesis**를 준비하고 hash를 고정한다.
키를 생성한 탭을 새로고침하지 않는다. 새 genesis/홈의 실제 계정 조회가 연결될 때까지
UI의 예치/출금은 미구현으로 표시한다. 기존 S1 genesis/home을 재사용하지 않는다.

잔고는 atoms 문자열이며 P를 A에 더하지 않는다. 체결은 PENDING/CORRECTED이고
LOCAL_ACCEPTED는 LOCAL_FSYNC/replicated=false다. REST 1초 polling에서 5초 경과 또는
로그인 만료/단절에 신규 주문을 닫는다. 재접속은 재인증 및 전체 페이지 snapshot이다.
동일 seq 페이지가 바뀌면 부분 내역을 게시하지 않는다. UNKNOWN은 같은 서명 바이트와
ID의 영수증 조회·명시적 재시도로만 해소한다. 키를 잃은 탭은 복구할 수 없다.

현재 시험은 합성 API와 실제 ML-DSA/공통 벡터를 사용한다. 실제 체인·4검증인·브라우저
인수, S3 정산, WS, 분산 내구성 및 처리량 증거로 승격하지 않는다.
