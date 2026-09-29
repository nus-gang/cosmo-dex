# S0-G rc3 독립 재시험

`RETEST-RC3.md`와 `evidence/verdict.json`이 이번 판정이다. rc2의 `REVIEW.md` 및 `evidence-rc2/`는 과거 FAIL 증거다.

새 checkout에서 전체 Git history와 승인 C/D/E/F SHA를 준비한 뒤:

```sh
python3 security/prepare.py
bash security/run.sh
```

`prepare.py`는 기존 컴포넌트 디렉터리가 있으면 중단한다. C/D/F 전체 protocol 파일을 rc3와 비교한다. E는 원래 rc2 manifest를 보존하고 실제 소비한 schema/message-codec/s0-cases/batches 네 파일이 rc3와 같은지 검사한다. E 전체 manifest가 rc3라는 주장은 하지 않는다.

Go 1.24.4, Rust 1.92.0, Node 24.21.0, Python 3.14.0. macOS에서는 설치된 Rust toolchain을 RUSTUP_HOME과 RUSTUP_TOOLCHAIN으로 선택했다. 캐시는 소스/lock을 변경하지 않는다.

run.sh는 기존 구현 시험·정산 독립 20조건·ML-DSA 3×3·codec/정수/만료/도메인 부정 시험·rc3 공통 fee/cap/합성 정책 및 새 서명으로 결합한 판정 시험을 실행한다. 실패를 exit 1로 보존한다. 기존 G-04 사례는 rc3의 분리된 실제 decision 포트를 사용한다. 합성 전제만 넣은 evaluate_snapshot 결과는 암호 실행 결과가 아니다. signed 사례는 실제 authenticate/admit 포트를 호출한다. adapter는 검증 결과를 고쳐서 반환하지 않는다.

Security가 작성한 harness는 CEO 검토와 H 독립 재현 대상이다. ACK/WAL/원장/REST/WS/체인 연결·실자산 사용은 하지 않았다.
