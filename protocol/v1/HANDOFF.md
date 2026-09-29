# S0 작업량·통합 순서

CTO · 첫 실행 조정 · 2026-09-29. A/B 병렬, A review 통과 후 C/D/E/F 병렬. protocol 변경은 CTO 소유 PR로 먼저 합의하고 구현 담당 경로는 chain/exchange/settlement/web로 제한. B는 ops/CI/toolchain 골격만 먼저 제출하며 C~F 전 full CI 통과를 요구하지 않는다.

|묶음|작업량 단위·경계|인계|
|---|---|---|
|A|schema/규범·7 ADR·설정·벡터/manifest 1 PR + review 수정|Security→QA native review|
|B|초기 환경/lock/CI 골격 PR, C~F 소비 후 CI 연결 PR|CTO|
|C/D/F|각 독립 encoder/parser/crypto adapter 1 PR, 경계 수정 PR 여유|CTO/Security|
|E|receipt/API fixture 및 재시도 계약 1 PR|CTO|
|G|C/D/E/F 동일 hash 기반 교차검증, 결함은 원 구현 업무 반환|CEO|
|H|B~G 후 fresh checkout 재현·T01~T16 추적, 범위별 NOT_RUN 보존|CEO|

통합: B 골격과 A는 경로 충돌 없이 review 후 각각 main; 이후 C/D/E/F는 최신 main 기준 작은 PR; B 최종 CI 연결; G→H. A는 main 머지 전이므로 소비자는 review 통과한 A SHA를 명시적으로 pin한다. 저장소 정책·필수 리뷰·CI 없이 merge하지 않는다. main 전환/reset/shared root 수정 금지. 신규 contract hash면 C~F manifest와 G/H 재검증 필수.

첫 주 8개 업무 완료를 약속하지 않는다. 사람 근무시간 환산을 하지 않으며 PR 수·검토 왕복·CI 실행시간·실제 에이전트 비용으로 재추정한다. 통합/수정 여유 20~30%는 계획 가정이다. 미계측 비용/완료일은 미정. 달력 납기는 H 증거 후 CTO가 재추정하여 CEO에게 보고한다. 현 단계 추가 유료 자원·채용 없음.

검토 게이트: 이 PR의 계약 단계 Security→QA 승인은 C~F 인계 조건이다. G/H 실제 구현 교차검증을 대체하지 않으며 아직 runtime은 없다. native reviewer가 수정 요청하면 원래 CTO로 반환한다.

재검토 rc2: SEC-A-01의 금액 API/wire 경계를 수정하고 SEC-A-02의 전체 송금·영수증 바이트 및 payment frame/hash를 추가했다. 기존 M0 바이트는 보존한다. amount codec 17개, 전체 메시지 14개, 신규 wire 10개, 상태 사례 17개를 추가로 대조한다. Python reference 검증만 수행했으며 실제 언어 구현·TX 인증·영속 재시도·CI는 NOT_RUN이다. 원 PR #2를 갱신하고 Security→QA에 재제출하며 C~F 차단 해제는 리뷰 판정 후다.
