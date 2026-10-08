# Chain 시작 IPC 경계

2026-10-07 · NUS-73 · 서비스 미기동

start는 B 입력/home 검증 → writer lock → home 재검증 → CHAIN_READY\n 출력 → 최대 30초 START\n+EOF 대기 → home 재검증 → DB/node 생성 순서다. 부모는 START 전에 인증된 독립 승인과 실행 bytes를 재확인해야 한다. IPC 자체는 승인이 아니다. preflight 동작은 유지한다.

EOF·잘못된/추가 바이트·시간 초과·취소·READY 출력 실패는 DB 생성 전에 거절한다. stdin은 gate 종료 시 닫는다. SIGINT/SIGTERM은 READY 대기부터 노드 종료까지 같은 context로 전달한다. writer lock은 거절 반환에서도 해제한다.

Go1.26.5, GOPROXY=off/GOSUMDB=off/GOTOOLCHAIN=local, 기존 GOMODCACHE/GOCACHE, go test -mod=readonly -tags dev_local_demo ./cmd/nus-s3-local-chain -count=1 -v: 신규3+기존9=12 PASS/0 FAIL. 순수 IPC reader/pipe 및 기존 filesystem 시험이다. 실제 child START/DB/서비스/listener/RPC0. 실제 신호/부모 인증 및 managed chain launcher 연결은 남아 있다. runtime pin 미발급·DEV NOT_RUN·durable_ack=false. 원 G00/ACK/부모 blocker 유지.

다음 SRE 작업: chain private 실행 사본/입력 capture·인증 부모·관리 command·fault/정리 및 최종 build/manifest/독립 승인/CTO→Security.

검증 임시 home은 Go 시험 종료 시 제거됐다. 신규 전용 build cache/binary 없음. 활성 공유 cache는 보존한다. 게시 후 run-owned API snapshot만 삭제하고 전후 bytes를 댓글에 기록한다.
