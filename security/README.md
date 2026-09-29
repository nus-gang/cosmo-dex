# S0-G 독립 보안 검증

판정은 [REVIEW.md](REVIEW.md), 기계 판정은 [verdict.json](evidence/verdict.json)이다. 리뷰 완료와 제품 PASS를 구분한다. 제품 컴포넌트를 수정하지 않았으며 `security/`의 adapter는 시험 전용이다. Rust 서명 generator는 라이브러리 `fips204` 호출이고 Exchange 런타임에 서명 기능이 있다는 뜻이 아니다.

새 격리 checkout에서 전체 Git history와 C/D/E/F 커밋이 필요하다. Python 3.14, Go 1.24.4, Rust 1.92.0, Node 24.21.0으로 실행했다.

```sh
python3 security/prepare.py
bash security/run.sh
# 현재 고정 입력은 policy 차이로 exit 1: evidence/summary.json 확인
# 브라우저 재현(로컬 설치 경로 지정)
cd web
CHROME_BIN=/usr/bin/google-chrome npm run test:browser
```

`prepare.py`는 기존 component 디렉터리가 있으면 덮어쓰지 않고 중단한다. 공유 root에서 실행하지 않는다. 입력은 `evidence/inputs.json`의 정확한 SHA이고 각 SHA의 protocol 38파일이 rc2와 같은지도 검사한다. OS의 toolchain 경로가 다르면 `RUSTUP_HOME` 등 정상 도구 설정을 먼저 구성한다. 캐시 경로를 재사용할 수 있으나 소스·lock은 고정한다. 로컬 실행에서는 Go 모듈만 기존 다운로드 캐시를 읽고 Rust/npm 의존성은 잠금 파일로 다시 설치했다.

`run.sh`는 기존 구성요소 시험, 독립 정산 20조건, 실제 3언어 adapter 빌드, 420개 differential 비교를 실행한다. 기존 시험이 실패하면 즉시 중단한다. 마지막 비교의 7개 차이는 보고서의 의도한 검토 결과이며 오류를 성공으로 숨기지 않는다. 신규 CI도 같은 exit 1과 raw artifact를 전달한다. GitHub CI 실제 실행 여부는 Paperclip work product/실행 링크를 확인한다.

전체 JSONL·420결과·공개 생성 서명·재현 요청은 Paperclip artifact에 보관한다. 저장소에는 핵심 요약과 재현 코드를 둔다. 기존 부정 fixture의 nonempty-context 3건은 명시된 verifier context를 공급한다(Go는 라이브러리 raw context 시험). 별도로 3언어에서 nonempty-context 서명을 생성하여 empty-context 검증으로 모두 거절함을 확인한다. 이를 empty-context 제품 API가 임의 context를 받는 것으로 해석하지 않는다.

이 시험 harness도 Security 작성 코드이므로 CEO 네이티브 review 및 H의 독립 재현 대상이다. 본인의 코드·판정을 독립 승인했다고 주장하지 않는다.
