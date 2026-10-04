# S2-C REST 입력 경계 체크포인트

SignedCommand wrapper의 16384B 제한·중복/추가/누락/null 거절, manifest snapshot context 정확 비교, 정규 base64·3309B 서명 길이·Order/Cancel canonical wire 검사를 추가했다. LocalAction은 request_id만 받고 canonical bytes를 기존 binding에 전달할 수 있다. Origin은 승인된 두 loopback 값만 정확 일치 허용한다.

이 모듈은 파서이며 인증 증거가 아니다. 세션 owner 확인·서명 검증은 호출자가 기존 sequencer와 연결해야 한다. HTTP가 chunk/body를 읽는 동안의 크기 제한, 중복 Origin header 거절, nonce/session 인증, RPC 연결·프로세스 서비스는 미완료다. parser 결과를 곧바로 인증된 요청으로 취급하면 안 된다. 실제 서명 fixture를 입력했지만 이번 시험은 서명 검증 재시험이 아니다.

시험: 신규 2개 PASS(0.03초), all-targets clippy PASS(1.19초). 기존 시퀀서 40개 재실행 없음. macOS xcrun sandbox 경고가 출력되었으나 실행은 성공. 원격 CI 미확인. 처리량/CPU/RSS 미측정, 유료 비용 없음. 제품 PASS 또는 CTO→Security 검토 요청이 아니다.

재현(저장소 루트):
```
export PATH=/Users/gangdongju/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH CARGO_HOME=/Users/gangdongju/.cargo RUSTUP_HOME=/Users/gangdongju/.rustup
cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_request
cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings
```
