# rc3 공통 CI와 검증 범위

B 초기 골격 이후 H-01 연결. ops/ci/manifest.json은 CTO가 고정한 A/C/D/E/F commit,
계약·전체 벡터 집합 hash, 파일별 hash, 실제 명령, 필수 ID·원본 기대값·범위를 담는다.
E 원본 rc2와 실행 rc3는 e_compatibility에서 구분한다. E의 읽기 입력 3개 hash와
schema/fixture 재생성 불변성도 매 실행 확인한다.

## 재현

Go 1.24.4, Rust 1.92.0, Node 24.21.0, Python 3을 설치한 새 checkout에서:

    make bootstrap
    python3 tests/test_vector_gate.py
    python3 tests/test_runtime.py
    make vectors

실제 git SHA, manifest hash, UTC 시작/종료 시각, 명령/소요시간/원시 stdout·stderr가
.evidence/vectors.json과 개별 로그에 남는다. workflow artifact로도 업로드한다.
새 입력을 승인받은 경우에만 python3 ops/ci/build_manifest.py로 manifest를 재생성한다.
CI는 --check로 고정 oracle와 manifest가 일치하는지 검사한다.

## 실행 경계

ops/ci/ports/는 고정 소비자 라이브러리를 호출하는 SRE 어댑터다. 소비자 소스는 변경하지 않는다.
JSON 배열 stdin의 각 요청을 실행하여 {id, actual} 배열을 반환한다. gate는 정확한 ID 집합,
중복·누락·추가, 자료형, canonical bytes/frame/실제 서명 검증 결과와 오류를 각각 oracle에 대조한다.
다른 언어와 같은 값이라는 이유로 통과시키지 않는다. receipt의 revision/hash는 검증된 파일 입력에
결합하여 gate가 작성하며, 이 메타데이터가 소비자 스스로 보고한 내용이라고 해석하지 않는다.
legacy Rust/TS receipt의 서로 다른 revision 표기를 묵시적으로 동일시하지 않는다.

| 집합 | 검증 포트/범위 |
|---|---|
| 서명 3 긍정/35 부정 | 실제 ML-DSA; canonical 3 및 frame 3도 각각 oracle 대조 |
| amount 17, message bytes 14, wire 18 | 세 언어 실제 codec 반환값 |
| fee 17 / cap 21 | 세 언어 실제 산술·cap 함수 |
| decision 34 | 세 언어 합성 snapshot policy; auth 주입, 실제 인증 성공 아님 |
| API 오류 6 | Rust/TS API mapping. Go API mapping은 제공되지 않아 이 집합에 포함하지 않음 |
| 등록/서명/만료/정수/IOC 등 | 고정 구현의 전체 component assertion 시험과 원시 로그 |
| integers.tsv 32 | Go/Rust component 시험. I20만 rc3의 fee(0,0)=0 override; 원본 보존 |
| E receipt/API | 기존 rc2 소스 + rc3 입력에서 16 모의 회귀 및 생성물 불변성 |
| batch 경제 상태 / message state | protocol reference checks만 사용. 제품 실행 PASS 아님 |

CTO required-cases.json의 243개 id 레코드는 중첩 snapshot id까지 포함한 원본 인덱스다.
독립 테스트 243개라는 의미가 아니다. amount(17)와 API(6)는 원본에 id가 없어 별도 ID를 부여했다.
coverage는 이 원본 인덱스와 실행 집합을 연결한다. component assertion은 개별 반환값 영수증과 구분한다.
G/H는 component 내부 assertion의 충분성 및 독립 교차 생성/검증을 별도로 판정한다.

PASS는 위 S0 CI 범위에 한정한다. ACK/ledger는 NOT_CONNECTED, WAL replay는 NOT_RUN,
실제 체인·4검증인 runtime은 미연결이다. 실제 등록 상태·DB·보존식·출금 경합 시험 또는
전체 제품 통합 PASS가 아니다. main merge 승인도 별도다.
