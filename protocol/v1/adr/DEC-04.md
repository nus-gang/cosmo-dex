# DEC-04 거래 수수료

2026-09-29 · 기술 결정 CTO · 근거 담당 Chain·Exchange.
상태: S0 테스트 전용 결정, 네이티브 Security→QA 계약 검토 대기. 승인 후 C~F 소비, G/H 최종 검증은 별도다. 목표 시점: A 계약 리뷰 완료, C~F 착수 전. 달력 납기 아님.

수취 자산 공제, 정책 1=0bps/2=25bps, maker/taker 동일, fill별 ceil. 제3자산 과금은 보존식 복잡화로 제외.

M0 r2와 Security r1를 재사용한다. 구체 수치는 승인 Plan 3판의 S0-A 위임에 따른 합성 설정이며 경제/실자산 정책으로 확장하지 않는다. 상세 규범·오류·한도는 CONTRACT.md 및 dev-config.json. 변경 이유: 준비도 검토의 미정/불일치를 동일 입력·판정으로 고정.

검증 근거: manifest 해시, tools/check.py 결과. 이번 단계의 자체 검사는 독립 Security/QA 승인을 대신하지 않는다. 검토 결과는 [NUS-10](/NUS/issues/NUS-10)의 네이티브 execution review에 기록한다. 판정/시각은 review 전 미기입이며 승인으로 미리 표기하지 않는다.

## rc3 결정 — G-02/03

수수료 helper는 receive U128 → active bps 0..10000 → 0bps 분기 → 양수 ceil 및 fee>=receive 거절로 고정한다. 서명 cap은 U32 전 범위를 허용한다. 활성 비율과 cap을 분리하면 schema/wire를 보존하면서 active<=cap 및 활성 비율 제한을 모두 강제할 수 있다. Rust의 cap<=10000 추가 정책은 공통 정책으로 채택하지 않는다. 이는 기존 harness의 OK를 추인한 것이 아니라 CEO 반환에 따른 CTO의 명시적 결정이다. BPS_RANGE와 API 매핑, 경계 벡터는 DECISION-PORT.md 참조. 테스트 전용이며 독립 재검토 전이다.
