# S0-G rc4 독립 재시험

최종 rc4 기준은 `RETEST-RC4.md`와 `evidence/`이다. 기존 rc2, rc3 및 fixed 실패 결과는 각 보고서와 `evidence-rc2/`, `evidence-rc3/`, `evidence-fixed/`에 보존한다.

통합된 새 checkout에서는 `bash security/run.sh`를 실행한다. 현재 checkout의 C/D/E/F 코드를 직접 빌드하며 다른 브랜치의 코드로 교체하지 않는다. CI는 모든 PR과 main push에서 실행하고 실행 SHA와 source tree를 기록하며, 저장된 과거 evidence를 지운 뒤 새 결과만 업로드한다.

`prepare.py`는 구성요소가 없는 과거 보안 전용 브랜치를 재현할 때만 사용한다. 통합 checkout에서는 기존 구성요소를 발견하면 의도적으로 거절한다. 원본 C/D/E/F commit은 `evidence/inputs.json`과 `ops/ci/cto-input.json`의 역사적 출처 기록이다.

Go 1.24.4, Rust 1.92.0, Node 24.21.0, Python 3.14. macOS에서는 RUSTUP_HOME=/Users/gangdongju/.rustup RUSTUP_TOOLCHAIN=stable을 지정했다. 브라우저는 web에서 EVIDENCE_DIR=../security/evidence/browser npm run test:browser. Linux는 workflow의 Chromium 설치 및 CHROME_BIN을 사용한다.

rc4는 969개 비교를 수행한다. 872행은 전체 응답, 97행은 지정된 기대 필드를 비교한다. 실제 서명과 합성 정책은 분리하며 harness는 차이를 exit 1로 보고한다. 실제 ACK/WAL/원장/체인/REST/WS와 제품 T01~T16은 NOT_RUN/NOT_CONNECTED다.
