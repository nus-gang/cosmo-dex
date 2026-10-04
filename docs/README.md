# 문서 목차

S2의 두 자산·주문은 [S2 시작 안내](s2-quickstart.md), 기동·복구는 [S2 운영](../ops/s2/README.md)을 따른다. 검증된 main `aad654bcf6760bc9af162b681a5996487ffc715e`의 완료된 인수와 제한은 [검증 기록](verification.md#s2-main-인수)에서 확인한다.

S1을 처음 실행한다면 [S1 사용자 시작 안내](quickstart.md)를 따른다. 이 경로는 제품 기준 main `32781aa97d62ec747e7a25c10fdb8b58030d79f2`의 S1 재현을 보존한다.

| 독자·목적 | 시작점 | 권위 있는 상세 정의 |
|---|---|---|
| 사용자: S2 예치·주문·취소/IOC·복구 | [S2 시작 안내](s2-quickstart.md) | [S2 계약](../protocol/s2/CONTRACT.md), [운영](../ops/s2/README.md) |
| 사용자: 설치와 두 계정 100→40→60 | [사용자 시작 안내](quickstart.md) | [브라우저 동작과 시험](../web/s1/README.md) |
| 전체 구성과 지원 범위 | [구성·데이터 흐름](architecture.md) | [S2 계약](../protocol/s2/CONTRACT.md) |
| 개발자: API·서명·금액 | [API 안내](api.md) | [S2 REST](../settlement/s2/README.md), [Chain 앱](../chain/app/README.md) |
| 운영자: 시작·중지·로그·재시작 | [S2 4검증인 운영](../ops/s2/README.md) | [runtime.py](../ops/s2/runtime.py), [공개키 초기화](../chain/app/USER-PUBLIC-KEYS.md) |
| 기여자: 버전과 문서 영향 | [기여 지침](contributing.md) | [CI workflows](../.github/workflows/) |
| 검토자: 적용 SHA·증거·역사적 실패 | [검증 기록](verification.md), [문서 인벤토리](inventory.md) | [변경 기록](CHANGELOG.md) |
| S0 재현 | [S0 main 통합](s0-main-integration.md), [벡터 인터페이스](vector-interface.md) | [protocol/v1](../protocol/v1/README.md), [보안](../security/README.md) |

[S0-B 초기 개발 기록](development.md)과 [M0 운영 설계](../ops/OPERATIONS-M0.md)는 역사·설계 자료다. 현재 S1의 설치 안내나 운영 성능 보장으로 읽지 않는다. 규약 변경은 CTO와 구현 담당자가 결정하고, 문서는 규약·schema·코드의 원본을 연결한다.
