# Status revision 예약기 체크포인트

journal OS writer lock 아래 응답 revision 범위를 fsync+rename+directory fsync 후 반환한다. 재시작은 사용하지 않은 번호도 건너뛴다. engine command seq/commit/outbox는 바꾸지 않는다. 누락/손상/문맥 불일치/소진/불확실한 임시 파일은 초기화하지 않고 handle을 차단한다.

새 journal에 status.revision을 생성한다. 기존 journal에는 자동 생성/업그레이드하지 않는다. 아직 Service Status 투영과 연결하지 않았으며 기존 Status revision 문제는 미해결이다. 후속 작업에서 recovery API·Status/LedgerView에 연결하고 오류 전파·역순/재시작·프로세스 crash 시험을 추가해야 한다. 독립 파일은 주문 자산 상태가 아니며 로컬 관측 응답의 순서만 예약한다.

검증: journal 13개 PASS(신규 2개 포함, 1.37초), all-targets clippy PASS. 최초 cargo 실행은 cache 환경 미설정으로 네트워크 조회 실패; 기존 CARGO_HOME을 지정해 --offline --locked로 성공. 처리량/CPU/RSS 미측정, 원격 CI 미확인, 유료 비용 없음. 제품 PASS 또는 전문 검토 요청이 아니다.

재현: PATH=/Users/gangdongju/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH CARGO_HOME=/Users/gangdongju/.cargo RUSTUP_HOME=/Users/gangdongju/.rustup cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_journal
