# 웹 HTTP 연결 경계 — NUS-73

`web_connection.handle_connection`은 기존 `WebProxy`와 `StaticWeb`을 HTTP/1.1 bytes에 연결한다. `serve_listener`는 승인 후 호출자가 생성한 IPv4 `127.0.0.1:5173` TCP listener를 소유하고 순차 처리한다. 모듈 import/시험에서 bind/listen/connect는 실행하지 않는다. 조직 승인이나 관리 runtime 등록을 대신하지 않는다.

- 요청 line 4096 bytes, 전체 header 16384 bytes/32개, body 16384 bytes. ASCII·CRLF·canonical Content-Length를 요구하며 중복 헤더를 보존한다. Transfer-Encoding/Expect/Upgrade, POST 길이 누락, 다른 HTTP 버전, absolute target은 거절한다.
- 연결당 요청 한 개, response `Connection: close`. 뒤따르는 pipelined 요청은 실행하지 않는다. 총 연결 IO 5초, upstream 자체 2초 상한. 정지/역행 시계/IO 오류/interrupt에서 연결을 닫는다. 응답 유실 뒤 재시도0이며 False/503은 미방송이나 확정 실패 증거가 아니다.
- listener는 명시적 최대 요청 수 1–10000, 수명 1–3600초를 요구한다. 동시 처리0. idle accept는 최대 200ms 단위이며 파싱/연결 오류 뒤 새 accept0. 수명 도달·정지 뒤 이미 받은 socket도 닫는다. 실제 프로세스/포트 종료 입증과 구분한다.

검증: `PYTHONPATH=ops/s3-local python3 -m unittest test_web_connection test_web_proxy test_web_upstream test_static_web -v`.
신규 연결/루프8 + 기존 proxy/upstream/static13 = 21 PASS/0 FAIL. 메모리 stream/listener 및 합성 upstream만 사용했다. 실제 HTTP/browser/Chain 시험, Rust/Go 전체 회귀는 미실행이다.

남은 연결: 승인 manifest의 웹 bytes/Context와 관리 gate, 실제 listener 생성/CLI 및 runtime 등록 packet. 이후 새 fee0/25 home/genesis·chain/fault/정리·다섯 descriptor 최종 build/manifest·CEO/CTO 독립 승인·CTO→Security 심사. 실제 서비스 기동은 승인 pin 이후 L-T에 남긴다. 현재 pin 미발급·DEV NOT_RUN·durable_ack=false·€0.
