# 문서 목차

처음 실행한다면 [S1 사용자 시작 안내](quickstart.md)를 따른다. 문서의 제품 기준은 main `32781aa97d62ec747e7a25c10fdb8b58030d79f2`이며 새 문서 검토·병합 상태는 [검증 기록](verification.md)과 Paperclip DOC-1에서 구분한다.

| 독자·목적 | 시작점 | 권위 있는 상세 정의 |
|---|---|---|
| 사용자: 설치와 두 계정 100→40→60 | [사용자 시작 안내](quickstart.md) | [브라우저 동작과 시험](../web/s1/README.md) |
| 전체 구성과 지원 범위 | [구성·데이터 흐름](architecture.md) | [S1 계약](../protocol/s1/CONTRACT.md) |
| 개발자: API·서명·금액 | [API 안내](api.md) | [REST](../settlement/s1/README.md), [Chain 앱](../chain/app/README.md) |
| 운영자: 시작·중지·로그·재시작 | [4검증인 운영](../ops/s1/README.md) | [devnet.py](../ops/s1/devnet.py), [공개키 초기화](../chain/app/USER-PUBLIC-KEYS.md) |
| 기여자: 버전과 문서 영향 | [기여 지침](contributing.md) | [CI workflows](../.github/workflows/) |
| 검토자: 적용 SHA·증거·역사적 실패 | [검증 기록](verification.md), [문서 인벤토리](inventory.md) | [변경 기록](CHANGELOG.md) |
| S0 재현 | [S0 main 통합](s0-main-integration.md), [벡터 인터페이스](vector-interface.md) | [protocol/v1](../protocol/v1/README.md), [보안](../security/README.md) |

[S0-B 초기 개발 기록](development.md)과 [M0 운영 설계](../ops/OPERATIONS-M0.md)는 역사·설계 자료다. 현재 S1의 설치 안내나 운영 성능 보장으로 읽지 않는다. 규약 변경은 CTO와 구현 담당자가 결정하고, 문서는 규약·schema·코드의 원본을 연결한다.
