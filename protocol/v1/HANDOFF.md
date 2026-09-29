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

## rc3 수정 반환 인계

[NUS-16](/NUS/issues/NUS-16)의 FAIL·7차이와 rc2 입력 SHA를 보존한다. G-01~04 수정 후 제품 재시험은 NOT_RUN이다. rc3은 수수료 순서/API 오류, U32 cap, 등록 타입 전달, 인증/합성 정책/ACK 포트를 추가한다. 기존 wire/서명 bytes·DEV 설정은 불변. Python 명세 예만 실행했다.

통합 순서: 원 PR #2 rc3 → Security→QA 계약 검토 → [NUS-12](/NUS/issues/NUS-12)·[NUS-13](/NUS/issues/NUS-13)·[NUS-15](/NUS/issues/NUS-15)가 동일 새 SHA/hash를 pin하여 수정·자체 시험·네이티브 검토 → CTO가 세 수정/검토 완료 증거를 확인한 뒤 기존 [NUS-16](/NUS/issues/NUS-16)을 원래 Security 담당자에게 resume → [NUS-17](/NUS/issues/NUS-17) 재현. 지금 G를 조기 resume하거나 C/D/F blocker를 해제하지 않는다. 중복 수정 이슈는 만들지 않는다. 실제 앱/WAL/원장과 후속 기능 단계는 범위 밖이다.

C/D/F 담당자는 수정/검토 완료 때 이 원 업무에 SHA·판정·검토 근거를 전달한다. CTO의 G 재개 조건은 세 업무의 실제 완료이며 이번 계약 검토 완료와 구별한다.

## rc4 / G-FIX-01 인계

기준: ADR G-FIX-01과 snapshot-output.json 60개 명세 예제. rc3 원본 vectors/schema/config 보존. 계약 검토는 새 Security→QA 네이티브 회차이며 과거 승인·FAIL 기록은 유지한다. 실제 제품 재시험은 NOT_RUN.

계약 승인 후 원래 업무로 반환한다:
- NUS-13 Exchange: binding 실패/부분 epoch 누락에서 snapshot null 치환을 제거하고 원본 ID와 정책 연결 판정을 분리한다. 전체 출력 60개 소비와 기존 회귀·독립 네이티브 검토.
- NUS-12 Chain: 완전하지만 모순인 binding의 CONTEXT_MISMATCH 출력을 NOT_CONNECTED/code=null로 맞춘다. 원본 ID 보존·누락·epoch 전체 출력과 기존 회귀·독립 검토.
- NUS-15 Wallet: 현재 코드상 관측 ID 보존·모순 미연결은 일치하지만 새 벡터 소비·같은 protocol hash와 full SHA에서 재검증/네이티브 검토를 제출한다. 검증 없이 구현 일치를 확정하지 않는다.
- NUS-11 SRE: A/C/D/F의 승인된 새 full SHA/path tree·contract/vector hash를 CI manifest에 고정한다. E는 실제 소비 파일 동일성만 확인하고 rc4 전체 구현으로 표기하지 않는다.
- CTO: C/D/F 수정·독립 검토가 모두 끝나면 새 통합 manifest를 업로드하고 기존 NUS-16을 원 Security 담당자에게 resume한다. 현재는 조기 재시험하지 않는다.
- NUS-16 Security: 기존 783개 ID/언어, 과거 783/782/1 FAIL을 보존하고 새 full-output 사례를 별도 집계한다. 기존 모순 사례는 ID를 유지하면서 rc4 전체 출력으로 강화한다. 전체 differential와 실제 암호 교차검증을 새 manifest에서 독립 재실행한다.
- NUS-17 QA: 같은 새 manifest와 CI 실행 SHA로 새 checkout 재현을 수행한다. 기존 FAIL/NOT_RUN 문서는 덮어쓰지 않는다.

작업량은 계약 60사례 추가, Rust binding 출력 수정, Go 모순 결과 정렬, TS 벡터 소비 및 3언어 검토/전체 재시험이다. 계약 승인 후 C/D/F는 각 checkout에서 병렬 가능하다. PR #2 계약 → C/D/F 원 PR → 새 통합 manifest/CI → G → H 순서로 검토한다. 통합 후보의 조립과 main 병합은 다르며 이 반환에 main 병합·출시 승인은 없다. 일정·비용은 실제 수정/검토·실행 결과 전 확약하지 않는다.
