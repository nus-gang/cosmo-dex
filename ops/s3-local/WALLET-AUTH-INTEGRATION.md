# 승인 Wallet 인증 API 통합 및 방송 필드 정렬

NUS-72 승인 head 720163e80cf239279d49ce58304fd3865e6bd684 / tree 1bc9b5ceca2d2f7b0330faf986c83c5a93a220bd의 web/s3 변경 6파일을 정확한 Git blob으로 SRE checkout에 반영했다. 기존 파일이 이전 승인 6077a839와 일치함을 먼저 확인했다. Wallet 원 checkout 변경0.

CTO revision cd4de405-94fd-4bbf-9f43-c13f3b752fd9, Security revision 1ca1d7d8-d61c-488f-b048-847913cf2727, native final decision 24828ee3-ae25-4707-bf30-d5773437035e / done·completed·approved 확인.

SRE chain_broadcast parser와 router/실제 인증 거절 fixture를 tx_bytes로 정렬했다. tx_base64만 보낸 요청은 인증·query·helper·방송 전에 거절한다. Wallet 프로토콜/lock/경제 로직 변경0.

검증:
- 승인 Wallet auth API 21 PASS/0 FAIL (합성 HTTP).
- 신규 Node boundary 1 PASS: 일회성 ML-DSA 키·실제 authenticated factory/login/withdraw가 생성한 정확한 본문·bearer 확인, UNKNOWN·방송1회. 공개 시험 데이터만 기록하며 private key 저장0.
- Rust 방송 adapter 4 PASS/0 FAIL (신규1+기존3). 위 생성 본문을 실제 parser에 전달하여 원 TX SHA/owner와 helper 입력 결합·전송 성공/오류 각각 callback1회·UNKNOWN을 확인. 인증/query/helper/방송 callback은 합성이며 실제 Go 서명 검증과 실제 REST/HTTP 종단을 입증하지 않는다. 나머지78 모듈시험 미실행.
- 최초 과거 rustc argv의 crate-root pub(super) 오류는 wrapper module로 보정했다. 실패/성공 로그 보존.

재현: NUS_BROWSER_BOUNDARY를 아직 없는 run scratch JSON 경로로 지정하고 node --experimental-strip-types --test ops/s3-local/browser-broadcast-boundary.test.ts 실행. 이어 build-fixed.json rustc argv로 wrapper를 컴파일하고 같은 환경변수에서 broadcast-tests chain_broadcast::broadcast_tests:: --test-threads=1 실행. CARGO_MANIFEST_DIR은 checkout/exchange. 도구/cache/rlib 경로는 기록된 현재 설치본이다.

base eb935f7 위 누적 미커밋 SRE 소스와 이번 exact Wallet blob을 artifact에 보존한다. runtime pin은 미발급, CTO→Security 제출 전이다. 다음 SRE 작업: 실제 인증 결과/정상 helper 종단, 웹 entrypoint, 새 fee0/25 home/genesis, chain/web launcher·fault/정리, 최종 manifest와 독립 승인. 실제 서비스/START/RPC/방송0·DEV NOT_RUN·€0. G00/ACK 및 표준 부모 blocker 유지.

## 세 route의 실제 어댑터 응답 왕복 보강

승인 Wallet의 authenticated LocalClient가 만든 account/broadcast/result 요청을 Rust 어댑터 subprocess에 그대로 전달하고 실제 반환 status/body를 Response로 돌려준다. 세 경로의 bearer 보존, GET 빈 본문, tx_bytes, tx_hash를 확인한다. 전송 callback 오류에도 효과 시도1회·SUBMISSION_UNKNOWN, 결과 조회 오류 뒤 UNKNOWN 및 추가 withdraw 거절·서명1회·방송1회 유지, 이후 exact TX/height/code 결과로 COMMITTED 전환을 검증했다.

Node 2 PASS(기존1+신규1), Rust 관련 회귀10 PASS(방송7+결과3). Rust bridge는 명시적 입력/출력 파일을 받는 ignored test이며 Node가 네 번 호출한 수를 독립 시험수에 합산하지 않는다. 실제 Go helper 시험1개는 ignored, 그 밖의 모듈시험 미실행. 인증/Account/proof/서명검증 callback은 합성이며 실제 인증 세션/Go 서명 검증/HTTP/체인 종단 PASS가 아니다. 실제 서비스/START/RPC/방송0, runtime pin 미발급.

재현: 기존 Rust chain_router 시험 build argv로 컴파일한 binary를 NUS_ROUTE_BINARY에 지정하고, 새 NUS_BROWSER_BOUNDARY 경로·PAPERCLIP_RUN_SCRATCH_DIR를 지정해 browser-broadcast-boundary.test.ts를 node --experimental-strip-types --test로 실행한다. 기록은 wallet-route-roundtrip/build.json·compile-final.log·roundtrip-final.log·방송/결과 회귀 로그에 보존한다.

남은 SRE 작업: 정상 Go helper/실제 인증 연결, 웹 entrypoint·새 fee0/25 home/genesis·chain/web launcher·fault/정리, 최종 build/binary/web SHA256·다섯 descriptor manifest·독립 승인 및 CTO→Security 심사. 승인 Wallet component만으로 runtime pin을 발급하지 않는다.
