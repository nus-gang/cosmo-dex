# S0 main 통합

이 통합은 이미 검토된 rc4 공통 계약·구현·CI·보안·QA 자료를 하나의 checkout으로 묶는다. 실제 체인·원장·REST/WS 연결, 제품 T01~T16, 후속 기능 단계와 실자산 운영은 포함하지 않는다.

## 원본과 포함 관계

| 범위 | 원본 PR / SHA | 통합 방법 |
|---|---|---|
| 공통 CI와 A/C/D/E/F | #10 / `178aaaf1253ef8573ec7158e02291d92b40e3ea3` | 전체 트리와 커밋 이력 포함 |
| 최종 보안 harness 및 QA rc4 보고 | #13 / `4f206ac6f6927152b8fd36f124ad4518de904d67` | #12와 이전 보안 이력을 포함하는 브랜치 병합 |
| 공통 계약 | #2 / `57c187f5474e02c3f624667d3b8380268e13dd1a` | #10의 `protocol/v1`에 원본 동일 파일 포함 |
| Go | #5 / `b0283833a0d040366219b35de58a3150d3956e92` | #10의 `chain/`에 원본 동일 파일 포함 |
| Rust | #4 / `7c7d0ca68b98f6c82fd297fa0dcd0a649194139d` | #10의 `exchange/`에 원본 동일 파일 포함 |
| 정산·API 모의 계약 | #3 / `f9b5bbf1e06a4a0c43bebc6d5bd081dc3b1892c2` | #10의 `settlement/`에 원본 동일 파일 포함 |
| TS 지갑 계약 | #6 / `5ff848415c3fda4e2285634950848c99824193be` | #10의 `web/`에 원본 동일 파일 포함 |

#1은 #10에 포함된다. #7/#9/#11/#12는 #13 이력에 포함된다. #8은 과거 실패 QA이며 최종 #13으로 대체된다. 과거 PR·브랜치와 첨부 증거는 보존하고 통합 완료 후 포함 또는 대체 사유를 남겨 열린 PR을 정리한다.

## 통합에 필요한 변경

- 제품 코드·계약·기대 벡터·보안 판정 함수는 원본을 유지한다.
- 보안 workflow는 모든 PR과 main push에서 현재 checkout을 직접 검사한다. 과거 별도 브랜치에서 필요했던 `prepare.py` 입력 추출은 실행하지 않는다.
- 보안 CI의 과거 결과 디렉터리를 비운 뒤 실행 SHA/source tree와 새 결과만 업로드한다. 과거 기록은 Git과 기존 첨부에 남는다.
- `README.md`와 보안 실행 안내를 현재 구성에 맞춘다. 과거 manifest의 candidate/NOT_RUN은 생성 당시 기록이므로 덮어쓰지 않는다.

## 재현과 완료 조건

Go 1.24.4, Rust 1.92.0, Node 24.21.0 및 Python을 준비한 새 clone에서:

```sh
make scaffold vectors
bash security/run.sh
cd web
EVIDENCE_DIR=../security/evidence/browser npm run test:browser
```

Linux의 Chromium 설치·`CHROME_BIN` 설정은 `.github/workflows/security-review.yml`을 따른다. 원본 비교에는 `ops/ci/cto-input.json`의 각 commit/path를 사용하며 A 44/C 24/D 42/E 13/F 43개 원본 파일을 비교한다. 추가 골격 파일은 원본 수에 포함하지 않는다.

통합 PR의 두 workflow 성공 및 CTO 검토 후 보호 규칙에 따라 merge한다. 실제 main SHA의 두 workflow 성공과 QA의 새 clone 독립 재현을 확인한 뒤 S0 main 통합을 완료 처리한다. PR 후보 성공만으로 main 검증을 대체하지 않는다. `qa-rc4/audit.py`는 과거 다중 checkout/CI 비교에 묶인 감사 스크립트이며 현재 main의 자동 게이트로 표기하지 않는다.

969개 교차 비교, rc4 60×3 전체 판정, 브라우저 453 checks는 S0 검증 범위다. 제품 T01~T16은 모두 NOT_RUN으로 유지한다.
