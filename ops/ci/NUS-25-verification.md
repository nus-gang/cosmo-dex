# S0/S1 manifest 경계 검증

- 기준: NUS-19 PR #16 head `21e669a0c7985fa27c073ee7ce771d5737e749ba`.
- 수정 전 `python3 ops/ci/build_manifest.py --check`: `manifest oracle/source mapping drift` 재현.
- 수정: 별도 S1 Go module인 `chain/app/`만 S0 파일 수집에서 제외. 임의의 nested module이나 `chain/application`은 제외하지 않음.
- `chain/`, `protocol/v1`, S0 T01~T16 원본 및 기존 CI workflow는 변경하지 않음. `.github/workflows/s1-chain.yml`의 별도 toolchain 검사, race test, build 유지.
- manifest 전후 비교: 키와 파일 목록 동일. `cases`, `coverage`, `lanes`, `boundaries`, source locks 등 모든 비해시 필드 동일. `files_sha256`에서 생성기 자신의 SHA-256만 변경.

## 실행 결과

- `python3 ops/ci/build_manifest.py --check`: PASS.
- `python3 tests/test_manifest_boundary.py -v`: 5 tests PASS. S1 파일 추가/변경/삭제 및 S1 부재 PASS; S0 소스·go.mod·벡터·schema·required-cases 변경/삭제 FAIL; 미승인 nested module·유사 prefix 추가 FAIL; oracle/coverage 변조 FAIL.
- `python3 tests/test_vector_gate.py`: 기존 9 tests PASS.
- `python3 protocol/v1/tools/check.py`: PASS (기존 reference-only 범위 그대로).
- `git diff --check`: PASS.
- 새 경계 회귀 검사는 `make test`에 연결되어 기존 S0 scaffold CI에서 실행됨.

## 검증 한계와 통합

전체 Go/Rust/TS 및 S1 실행 CI는 이 로컬 검증에서 실행하지 않음. 기존 workflow나 실패 판정은 생략하지 않으며 통합 후 원 executor/CEO가 PR CI 결과를 확인해야 함. 이 산출물은 S1 전체 완료 또는 운영 준비를 의미하지 않음.
