# S0-G 수정 후 독립 재시험

이번 결과는 `RETEST-FIXED.md`와 `evidence/`이다. 이전 rc3 759/754/5 FAIL은 `RETEST-RC3.md`, `evidence-rc3/` 및 PR #9에 보존한다. rc2도 기존 경로에 보존한다.

새 checkout에서 `python3 security/prepare.py && bash security/run.sh`를 실행한다. 고정 C/D/E/F commit은 evidence/inputs.json, CTO 원본은 evidence/cto-input-lock.json이다. E는 rc2 출처의 receipt/API 소비 4파일만 rc3 호환 확인한다.

Go 1.24.4, Rust 1.92.0, Node 24.21.0, Python 3.14. macOS에서는 RUSTUP_HOME=/Users/gangdongju/.rustup RUSTUP_TOOLCHAIN=stable을 지정했다. 브라우저는 web에서 EVIDENCE_DIR=../security/evidence/browser npm run test:browser. Linux는 workflow의 Chromium 설치 및 CHROME_BIN을 사용한다.

783 비교는 기존 759 + enum 정상값 4조합×3 + epoch 4조합×3이다. 실제 서명과 합성 정책은 분리한다. harness는 차이를 exit 1로 보존한다. Security harness는 CEO 및 H 독립 검토 대상이다. 실제 ACK/WAL/원장/체인/REST/WS와 제품 T01~T16은 NOT_RUN/NOT_CONNECTED다.
