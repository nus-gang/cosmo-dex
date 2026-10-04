# S2 Observation 조회 체크포인트

Service::observation_view(now)는 계약 Observation 필드를 투영한다. RPC 실패 후 마지막 성공 시각/latency를 보존하되 fresh=false. 재시작 후 수신 시각 0, last_success_age_ms=u64::MAX는 미관측을 뜻하며 catching_up=true/fresh=false이다. 시계 역행도 최대 age와 fresh=false로 표시한다. 알려진 block timestamp의 미래 허용은 기존 gate 규약을 따른다.

신규 시험 1개 PASS(0.09초), all-targets clippy PASS. 기존 36개 시험 재실행 없음. macOS sandbox xcrun 경고 존재. 기본 toolchain 경로 실행 실패 뒤 기존 RUSTUP_HOME/CARGO_HOME 지정으로 실행했다.

재현: RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer observation_projection
동일 환경에서 cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings

Status mode/reason/revision 전체 투영, 개인 조회·페이지, RPC/session/HTTP adapter, 최대 정정 용량과 프로세스 crash 통합은 미완료. LOCAL_ACCEPTED 서비스 인수·제품 PASS·전문 검토 요청 아님. 처리량/CPU/RSS 및 원격 CI 미측정/미확인. 유료 비용 없음.
