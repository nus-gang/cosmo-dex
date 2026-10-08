# 초기화 create 내부 실행 파일

`runtime/bootstrap_create.rs`는 부모 launcher용 `create-captured` child다. 인자는 순서대로 `--start-gate-fd FD --home ABS --evidence-root ABS --rpc-file ABS --rpc-sha256 HASH --capture-sha256 HASH --runtime-pin PIN --local-demo-profile ABS --acknowledge-unproven-space`다. 두 opt-in·exact capture SHA/EOF·C 입력 의미 검증을 기존 checker에 위임한다. RPC 파일은 C의 RPC cap·regular/no-link 검사와 exact SHA로 읽고 메모리에 고정한다. 연결된 AF_UNIX stream FD만 gate로 수용한다.

입력 검증 → 원 RPC no-replace/file+root fsync 보존 → READY → 최대5초 START+EOF 대기 → stop 재확인 → C create/drop 순서다. EOF/잘못된 신호/중단은 home 생성 전에 거절한다. 원문은 실패 후에도 보존한다. 성공 응답 유실 뒤 같은 home을 재생성하거나 자동 삭제하지 않는다. 부모는 유한 stdin과 전체 프로세스 수명 상한을 제공해야 한다.

이 IPC는 조직 승인이 아니다. 부모가 실행 바이트/descriptor와 최초 인증 audit를 확인하고 신뢰 로컬 RPC 원문을 획득해야 한다. READY 후 새 승인 audit 뒤에만 START를 전송한다. 이 전용 child의 descriptor/private 사본·인증 부모 launcher 연결은 아직 남아 있다. 임의 RPC 파일/SHA를 신뢰 조회 근거로 승격하지 않는다. 실제 조회와 서비스 기동은 L-T에서만 수행한다.

검증은 fee0/25 합성 descriptor/pin/RPC와 실제 C store를 사용한다. 내부 gate 승인 뒤 create/drop·두 번 reopen, gate 거절과 증거 충돌의 home 생성0, 실제 child READY 뒤 부모 EOF 거절·원문 보존, 잘못된 CLI의 열린 stdin 즉시 거절을 확인한다. 실제 child START 전송/서비스/listener/RPC0. 고정 오류만 출력하며 panic 내용은 비노출이다.

최종 build/다섯 descriptor manifest·독립 CEO/CTO 출처·CTO→Security 심사 이전이다. runtime pin 미발급·DEV NOT_RUN·€0. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 유지.
