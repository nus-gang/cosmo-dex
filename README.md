# cosmo-dex

S2 로컬 PoC는 한 호스트의 4검증인 개발망에서 두 시험 계정의 DEVBASE/DEVQUOTE 예치, 서명 지정가 주문·잠정 부분 체결·잔량 취소·가격 제한 IOC와 같은 home의 서버 기록 재생을 제공한다.

**[문서 목차](docs/README.md) · [S2 사용자 시작 안내](docs/s2-quickstart.md) · [S1 사용자 시작 안내](docs/quickstart.md)**

검증된 main `aad654bcf6760bc9af162b681a5996487ffc715e`에서 S2 인수가 완료됐다. A의 **10 DEVBASE**와 B의 **100 DEVQUOTE** 예치부터 주문·취소·출금 보류·재시작까지 [시작 안내](docs/s2-quickstart.md)를 따른다. 체결은 잠정이며 수취 자산 P는 재주문·출금에 사용할 수 없다. [적용 SHA·독립 QA·제약](docs/verification.md#s2-main-인수)을 확인한다.

- [현재 기능·구성과 데이터 흐름](docs/architecture.md)
- [API·서명·금액 규약 안내](docs/api.md)
- [개발 환경과 기여 지침](docs/contributing.md)
- [S2 개발망 기동·종료·복구](ops/s2/README.md)
- [적용 버전·검증 근거·제약](docs/verification.md) · [변경 기록](docs/CHANGELOG.md)

보존된 S1 안내의 제품 기준 SHA는 `32781aa97d62ec747e7a25c10fdb8b58030d79f2`다. S1에서는 계정별 **100 DEVQUOTE 예치 → 40 출금 → 확정 거래소 잔고 60**을 재현한다. S1/S2 모두 로컬 신뢰 RPC와 합성 자산만 사용하며 S2 내구성은 `LOCAL_FSYNC`·`replicated=false`다. 키는 브라우저 탭 메모리에만 있어 새로고침·종료 후 복구할 수 없고, 공개키 JSON은 백업이 아니다. S1에는 주문·매칭이 없으며 S2에서도 온체인 정산·외부 자산 게이트웨이·실자산 운영은 제공하지 않는다.

S0 재현은 [S0 main 통합 및 재현](docs/s0-main-integration.md), [runner 연결점](docs/vector-interface.md), [보안 재현](security/README.md)을 따른다. `make scaffold vectors`와 `bash security/run.sh`는 S0 검증이며 S1 사용자 시연이나 제품 T01~T16 전체 통과를 뜻하지 않는다.
