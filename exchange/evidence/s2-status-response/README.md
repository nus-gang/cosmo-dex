# S2-C Status 응답 revision 연결

Status와 LedgerView 응답을 직렬화하여 command seq와 별개인 단조 증가 revision을 발급한다. journal writer lock 아래 1,024개씩 fsync 예약하며 재시작은 미사용 번호를 건너뛴다. LedgerView의 revision은 내부 Status와 같고, 주문·fill entity revision과 book hash는 기존 committed sequence를 유지한다. 응답 지연/역순은 낮은 revision으로 구분한다. 실제 브라우저의 응답 폐기는 Wallet 통합 검증이 남았다.

예약 실패는 Status 성공 반환을 막고 recovery_required를 고정하여 신규 접수를 차단한다. 기존 receipt와 WAL 명령은 변경하지 않는다. 예약기는 첫 Status 요청 때 실행하므로 이 문서는 sidecar 검증이 서비스 생성 시 완료된다고 주장하지 않는다.

검증: 시퀀서 40개 PASS(2.43초), LedgerView 계약 oracle 5개 PASS, all-targets clippy PASS. clippy 첫 실행의 빈 range 경고는 Option 초기화로 수정 후 재시험했다. 처리량·CPU·RSS·원격 CI는 미측정/미확인. 추가 유료 비용 없음.

재현 환경: macOS arm64, Rust 1.92.0. RUSTUP_HOME=/Users/gangdongju/.rustup CARGO_HOME=/Users/gangdongju/.cargo, PAPERCLIP_RUN_SCRATCH_DIR에 쓰기 가능한 시험 디렉터리 지정.

```sh
cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer
cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings
python3 exchange/tools/check_s2_projection.py exchange/evidence/s2-status-response/views.json
```

남은 범위: 실제 서비스 프로세스/인증·RPC·HTTP 경계, 최대 정정 용량, 프로세스 crash 통합. 제품 PASS/전문 검토 요청이 아니다. CTO→Security 경로를 유지한다.
