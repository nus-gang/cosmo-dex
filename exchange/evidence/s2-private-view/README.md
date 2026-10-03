# 개인 조회 구성요소 체크포인트

owner별 C/R/D/P/A·자기 주문·자기 fill 투영 및 pagination을 구현했다. cursor는 owner/context hash(genesis 포함)/committed seq에 결합하고 오래된 seq·다른 owner·context·범위 초과를 거절한다. fill은 own_order_id만 제공하고 상대 주문 hash·서명·내부 outbox 필드는 제외한다. 서비스는 committed state만 전달한다.

신규 시험 1개 PASS(0.06초), all-targets clippy PASS, 계약 fixture oracle로 출력 LedgerRow/OrderView/FillView 검증 PASS. 기존 37개 시험은 재실행하지 않았다. 첫 시험은 출력 경로가 crate 상대 경로인 탓에 export 단계에서 실패했고 절대 경로로 수정 후 PASS. 기본 rustup 환경 실행 실패 뒤 기존 로컬 toolchain을 지정했다. macOS xcrun sandbox 경고 존재.

재현: RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer private_pages

아직 완전한 LedgerView/HTTP endpoint가 아니다. Status/revision·세션 인증·RPC/HTTP adapter·최대 정정 용량·프로세스 crash 검증이 남았다. owner는 검증된 세션에서 전달해야 한다. 페이지 생성은 전체 내부 상태 투영을 먼저 만들므로 메모리/성능 최적화 및 측정은 미완료. cursor는 인증 토큰이 아니며 조회 권한은 세션에만 있다. 원격 CI 미확인, 처리량/CPU/RSS 미측정, 유료 비용 없음. 제품 PASS나 전문 검토 요청이 아니다.
