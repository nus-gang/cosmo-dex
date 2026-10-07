# SRE 탭 진입점

승인 Wallet 구현을 수정하지 않고 `TabEntry`가 LocalKey 두 개와 authenticated client/mount를 조합한다. 기본 비활성이며 두 opt-in과 승인 origin이 필요하다. `registrations()`는 새 탭 키의 공개 데이터 복사본만 제공한다. 이 공개키를 새 genesis에 등록하는 초기화 단계가 뒤따라야 한다. 키 저장·복원·export는 없다.

`activate(root, suppliedContext, pinnedContext, fetch)`는 launcher가 공급한 exact Context와 문서 origin을 대조한 뒤 승인 Wallet UI를 연결한다. 자동 로그인/서명/TX/조회는 하지 않는다. pagehide/destroy 후 재사용을 거절한다. Context 인자 자체의 조직 승인 여부는 이 모듈이 판정하지 않는다. 최종 launcher/manifest가 제공해야 한다.

저장소 root에서 `node ops/s3-local/browser/build.mjs <절대 출력 디렉터리>`로 기존 esbuild를 사용한다. `entry-build.json`은 산출물 SHA256 기록이며 runtime pin이 아니다. HTML·정적 제공/proxy·공개키 genesis 초기화는 미연결이다.

`node --experimental-strip-types --test ops/s3-local/browser/entry.test.ts`: 4 PASS. 실제 Wallet 키/클라이언트/마운트와 최소 합성 DOM을 사용한다. 실제 browser/HTTP/chain DEV12는 NOT_RUN. 서비스 시작0, durable_ack=false, 표준 G00/ACK 유지.

## HTML과 정적 응답 준비 (2026-10-07)

`index.html` → bundled `page.js`가 기본 비활성 화면을 제공한다. 두 체크박스 이후 새 탭 키를 만들고 공개 등록 데이터만 표시한다. 같은 탭을 유지한 채 새 genesis 초기화를 마친 뒤 사용자가 화면 연결을 누르면 `/runtime-context.json`을 한 번 읽는다. 2초 abort·16KiB 응답 상한·redirect 거절·pagehide 취소와 늦은 응답 거절을 적용한다. 승인 C binding의 공개 Context 7필드와 64자리 hash/시장/버전을 요구한다. mount 후에도 자동 인증/조회/TX는 없다.

`static_web.py::StaticWeb`은 소켓을 열지 않는 순수 응답 경계다. 주어진 HTML/JS bytes와 공개 Context를 복사 고정하며 `/`, `/page.js`, 선택적인 `/runtime-context.json`만 GET/HEAD로 제공한다. exact Host·loopback peer·중복 Host 거절, no-store/nosniff/CSP를 적용한다. 경로 decode·파일 fallback·directory listing·REST proxy는 없다. Context 의미/승인 및 asset SHA 대조는 최종 launcher의 책임이며 이 함수는 승인 허가가 아니다. 검증 후 제공할 bytes를 constructor에 전달해야 한다.

`build.mjs`는 entry.js 외에 page.js/index.html을 생성하고 각각 SHA256을 entry-build.json에 기록한다. Context/genesis/키는 web build 집계에 포함하지 않는다.

검증: Node 신규 page4 + 기존 entry4 = 8 PASS, Python 정적 응답4 PASS, strict 타입검사·browser bundle PASS. 최소 합성 DOM/fetch와 메모리 응답 검증이며 실제 HTTP/browser DEV12는 NOT_RUN. 실제 listener·인증 REST proxy·관리 web command/승인 gate, 공개키 genesis 초기화는 다음 연결점이다. 이 정적 함수만으로 서비스를 시작할 수 있다고 주장하지 않는다.

## 동일 출처 REST 전달 경계 (2026-10-07)

`web_proxy.py::WebProxy`는 정적 응답과 승인 REST/ChainPort route를 분기한다. 원문 본문·bearer를 한 번만 고정 `127.0.0.1:<worker_port>`로 전달한다. Host는 worker 목적지로 바꾸며 Origin은 선택된 웹 origin과 정확히 일치해야 한다. 동일 출처 GET의 Origin 생략만 선택된 웹 origin으로 채운다. 브라우저가 명시한 다른 Origin·중복 Host/Origin/Authorization/Content-Length·전송 코딩·forwarded metadata·잘못된 route는 IO 전에 거절한다. 인증 판정은 기존 worker Rest가 수행한다. worker의 인증 실패 응답은 그대로 보존한다.

`web_upstream.py::Upstream`은 한 번의 literal loopback 연결과 총 2초 기한을 적용한다. send/recv마다 남은 시간을 설정하고 부분 IO를 처리한다. HTTP/1.1·최대 32헤더/각 4096바이트·정규 Content-Length·최대 2MiB JSON 응답만 받으며 redirect/chunked/cookie를 거절한다. 정상/오류/중단 모두 socket을 닫고 재시도하지 않는다. 응답 유실은 고정 503으로 반환하며 방송이 실행되지 않았다는 뜻이 아니다. Wallet의 UNKNOWN과 명시적 결과 조회 동작을 유지한다. bearer·원 요청·내부 오류를 로깅하지 않는다.

검증: 신규 proxy5+upstream4, 기존 static4 = 13 PASS/0 FAIL. 전송은 주입한 메모리 socket이며 실제 connect/bind/listen/HTTP/Chain RPC는 0이다. 실제 Wallet/worker Rust/Go 회귀는 이번에 재실행하지 않았다. 이 구현은 listener/HTTP 요청 parser·관리 web gate에 아직 연결되지 않았고 승인 허가를 발급하지 않는다. 다음 연결점은 bounded listener/요청 parser와 managed web command, 새 home/genesis·chain/fault/정리·최종 manifest·독립 승인이다.
