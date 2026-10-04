# DOC-1 문서 인벤토리

조사 기준: remote main `32781aa97d62ec747e7a25c10fdb8b58030d79f2` (2026-09-30 `git ls-remote`). 초기 공유 checkout `7942e93`은 기준으로 사용하지 않았다. 격리 브랜치 `docs/nus-34-s1-baseline`에서 정비한다.

| 원본 | 권위·기존 상태 | 누락·불일치와 이번 처리 |
|---|---|---|
| [README](../README.md), [목차](README.md) | 저장소 진입점, S0/골격 미연결 설명 | S1 지원 범위·역할별 경로·quickstart 연결 |
| Paperclip 사용자 안내 | 승인된 32781aa9 실행 안내 | [quickstart](quickstart.md)로 재사용; 원본 revision·검증 출처 보존 |
| [web/s1](../web/s1/README.md) | Wallet 기능·시험·세션 수명 | 후보 시점과 현재 main 구분; 전체 초보자 경로 연결 |
| [ops/s1](../ops/s1/README.md) | supervisor·포트·health/log·중단·원장 보존 | 재작성 없이 목차/quickstart에서 연결; fixture/사용자 키 구분 |
| [settlement/s1](../settlement/s1/README.md) | REST/journal·상태 의미·최초 실패 | 과거 미병합/미완료 문구를 당시 상태로 명시; 현행 main 근거 연결 |
| [chain/app](../chain/app/README.md), [공개키](../chain/app/USER-PUBLIC-KEYS.md) | SDK·CLI·genesis 구현 | 원본 유지; 권위 링크 및 fixture 주의 추가 |
| [S1 계약](../protocol/s1/CONTRACT.md), [schema](../protocol/s1/messages.proto) | 규범과 pin, 제안 당시 이력 포함 | 재정의하지 않고 [API 안내](api.md)에서 연결 |
| [S0-B 개발 기록](development.md) | 초기 버전·미연결 당시 설명 | 역사 표기 추가, 현재 [기여 지침](contributing.md) 연결 |
| [S0 통합](s0-main-integration.md), [protocol/v1](../protocol/v1/README.md) | S0 범위·고정 규약·원본 SHA | 보존; S1과 다른 범위임을 상위 목차에 명시 |
| [보안 기록](../security/README.md), [QA rc4](../qa-rc4/REPORT.md) | 독립 원시 증거와 판정 | 기존 기록 수정 없음; [검증 기록](verification.md)에서 최신 인수와 구분 |
| [M0 설계](../ops/OPERATIONS-M0.md), 원본 PDF | 설계 목표 | 현재 운영 기능으로 재서술하지 않고 원본 접근 경로 제공 |

새 문서는 구성·API 탐색, 검증 출처, 변경 기록, 문서 영향 기여 지침이다. 상세 규약·운영 명령·기존 보안 및 실패 기록의 사본을 새로 만들지 않는다. 중복된 실행 순서는 새 사용자의 재현에 필요한 최소 범위로 quickstart에 모으며 상세 시험은 원본으로 연결한다.

## S2-G 영향 조사 (최초 후보 당시)

[Docs 업무](http://localhost:3100/NUS/issues/NUS-42)의 기준은 F 승인 후보 `84ea152`/tree `eb436444d338cab5482bda29ad4333a981538b22`, 착수 원격 main `ec8961c`다. 기존 DOC-1 인벤토리와 S1 재현은 보존한다.

| 페이지/권위 입력 | 변화와 처리 |
|---|---|
| README, docs 목차 | S1만 있던 시작점에 S2 실험 후보 경로 추가, main 전달과 구분 |
| s2-quickstart (신규) | 설치 pin, 임시 웹 공개키 생성→새 home→통합 기동, 두 자산 예치·GTC/IOC·원장 수치·출금 제한·재시작 |
| quickstart, architecture | 기존 S1 적용 범위 명시, S2 데이터 흐름과 로컬 내구성 경계 연결 |
| api, contributing | S2 계약/schema/profile·공개/개인 조회·DIRECT 구분, 빌드 버전·CEO 통합 경로 |
| verification, CHANGELOG | F 승인·상속 시험·실패 이력·공유 preview 미검증·후속 main 인수 |
| ops/s2/runtime.py, README | init/serve/health, 포트·build pin·종료·로그 상한과 문서 명령 대조; 구현 변경 없음 |
| web/s2/index.html, browser.ts, browser.test.mjs | 실제 버튼·입력 단위·UNKNOWN·수동 순서·자동 시연 경계 대조 |
| protocol/s2, exchange/S2.md, settlement/s2 | 규범을 복제하지 않고 연결; 수치·상태·서명·cursor·보류 출금 설명의 원본 |

모듈 README의 단일 검증인/NOT_RUN·심사 후보 문구는 해당 구성요소 작성 당시 기록이다. 새 4검증인 F 증거는 verification에서 따로 연결한다. 원본 PDF·S0/S1 증거·역사적 판정은 변경하지 않는다.

## S2 완료 후 유지보수 (2026-10-04)

기준 main `aad654bcf6760bc9af162b681a5996487ffc715e` / tree `d96faa7e8bab62e71016f6e1c52c0411b4416d6f`. [CEO 후속 인계](http://localhost:3100/NUS/issues/NUS-42#document-post-s2-maintenance)와 [최종 인수](verification.md#s2-main-인수)를 대조한다. 기존 S1·후보 조사와 원시 실패 기록은 보존한다.

| 페이지 | 실제 차이와 이번 처리 |
|---|---|
| README, docs 목차 | 완료된 S2를 현재 시작점으로 표시; S1/S0 재현 유지 |
| s2-quickstart | 후보 SHA만 검증 main으로 변경; 명령·수치·동결 해제·키/원장 보존 절차 유지 |
| verification, CHANGELOG | main CI·독립 QA·최종 승인·HTTP 동시성 연결, OBS01 실패/성공 분리·NOTE01 측정 정정 |
| architecture, api, contributing | 현재 S2와 S1 고정 설명 구분; 후속 문서 CTO→QA→부모 CEO 정상 병합 경로 |
| ops/s2/README | 기존 통합 시험과 완료된 독립 main QA 구분, 사용자 안내 연결; 운영 명령 동일 |
| 관련 모듈 README·규범·시험 원문 | 구성요소 작성 당시 후보/NOT_RUN은 보존하고 현행 종합 근거에서 연결; 구현 변경 없음 |

검증 범위는 변경 링크·앵커·shell 구문, 검증 main 안내와 shell 블록 동일성, 규범/제품 diff 없음 및 새 PR CI다. 상속 QA를 이번 직접 실행으로 보고하지 않는다.
