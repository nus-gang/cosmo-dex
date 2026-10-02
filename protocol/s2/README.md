# S2 공통 계약 후보

[계약](CONTRACT.md) · [회고·추정·DEC](adr/S2-decisions.md) · [검증값](vectors/) · [schema](schema.json)

이 디렉터리는 실행 서비스가 아니라 B~J의 공유 입력이다. `manifest.json`의 hash와 정확한 PR head, NUS-36 네이티브 Security→QA 완료 여부를 함께 확인한다.

```sh
python3 protocol/s2/tools/check.py
cd chain
GOTOOLCHAIN=local go run -mod=readonly ../protocol/s2/tools/sign.go
```

Python은 표준 라이브러리만 사용한다. Go fixture 서명은 기존 chain의 CIRCL lock을 그대로 사용한다. 새 의존성/lock 변경 없음. fixture 생성용 공개 seed는 테스트 전용이다. `signed.json`은 합성 genesis에 결합되어 실제 예치/주문 성공의 증거가 아니다. 원본 S0/S1 검사도 유지한다.

Hash 규칙: 파일은 원본 UTF-8/LF bytes SHA256. 경로별 `(sha256 + 두 공백 + repo-relative path + LF)`를 ASCII 경로 오름차순으로 연결하여 SHA256. `manifest.json` 자신은 제외한다. manifest의 `files_sha256`은 모든 S2 파일 및 선택된 S0/S1 규범/원본 벡터/기존 lock을 포함한다. `contract_sha256`은 그 전체 집합, config는 profile.json bytes, vectors는 S2 vectors 하위 집합이다. 실행 code SHA/tree와 실제 genesis는 별도 runtime manifest에 기록하여 자기참조를 피한다.

생성 도구 변경은 계약 변경이다. 작성자만 명시적으로 `go run .../sign.go --keys`, `python3 protocol/s2/tools/generate.py`, `go run .../sign.go --generate`, `python3 protocol/s2/tools/check.py --seal`을 순서대로 실행한다. 소비자는 seal을 실행해 불일치를 숨기지 않는다.

rc2는 SEC-S2A-01 정정 용량 수정을 포함한다. `vectors/correction-boundaries.json`의 8개 모델은 1000/1001 fills, 200/201 lifetime 영향 orders, 0/25bps·양측 D/P/fee·누계·정정 후 C/E·단일 논리 commit/재생 효과 1회를 고정한다. 조회 페이지 상한은 그대로다. 이는 실제 서명 주문 실행·디스크 예약·crash 시험 결과가 아니며, 해당 시험은 C/F/H에서 수행한다.
