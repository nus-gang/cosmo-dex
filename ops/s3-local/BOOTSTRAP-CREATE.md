# 최초 RPC snapshot → C home 초기화

`runtime/bootstrap.rs`는 승인 C `Validated`의 Context로 한 번의 bounded loopback Snapshot 조회를 준비하고, ABCI 원문의 JSON-RPC id/code/height·정규 snapshot bytes를 검사한 뒤 `Engine::create`를 호출한다. C가 등록 owner·경제 보존·fresh bootstrap·guard fsync/no-replace·writer lock을 검증한다. genesis를 ledger로 재해석하지 않는다. C/L-D 소스 변경 없음.

`fetch`는 원 RPC bytes를 반환한다. 호출자는 실제 runtime 승인 확인 후에만 조회하고 원문을 증거로 게시한 뒤 `create`에 넘겨야 한다. `create`는 원문을 빌리며 exact snapshot bytes는 기존 C store가 보존한다. 이 내부 API 자체가 승인이나 원 RPC의 영속 게시를 보장하지 않는다. 현재 CLI/조직 승인 gate/원문 게시 연결은 미완료이며 실제 조회는 L-T 범위다.

검증: Rust 1.92.0 기존 rlib 사용 컴파일 PASS. 신규3+query 회귀11 = 14 PASS/0 FAIL/SKIP0. fee0/25 합성 snapshot/descriptor/pin으로 실제 C create, exact bootstrap bytes, writer2 및 기존 home 거절, 두 번 open/replay의 state/commit 불변 확인. 잘못된 id/version/code/height/base64/error/중복 JSON/비정규 bytes/과대 응답, Context/hash/잔고/terminal 목록 변조는 home 생성 전에 거절. 실제 fetch/RPC/listener/서비스0. 최초 fixture에 RPC 숫자를 C canonical encoder로 인코딩한 오류3개는 serde_json RPC 인코딩으로 수정; 실패 로그 보존.

다음 SRE 작업: 승인/증거 게시/CLI 초기화 연결·chain 관리 launcher/fault/정리·최종 build/다섯 descriptor manifest·CEO/CTO 독립 승인·CTO→Security. runtime pin 미발급, DEV NOT_RUN, G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false, €0 유지.
