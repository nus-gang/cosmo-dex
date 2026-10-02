# S2-C 예약 장부 체크포인트

2026-10-02, Exchange. 전체 서비스 구현은 진행 중이며 전문 검토 요청 전 단계다.

- owner/asset 공통 C/R/D/P와 A=C-R-D, 주문별 예약·fill별 기여분을 구현했다. 매 전이에서 전체 예약/체결 기여분을 장부와 대사한다.
- 매수 D는 한도가를 유지하고 P는 재사용하지 않는다. 취소/IOC/만료용 잔량 해제는 R만 바꾸며 체결 정정은 저장한 D/P 기여분을 역분개한다. 수량과 주문 ID는 부활시키지 않는다.
- 모든 변경은 후보 복사본에 적용한다. 금액·가격·누계·수수료·자기 거래 오류 시 양측 상태는 그대로다. 같은 fill ID 중복 적용은 거절하고 같은 정정 재호출은 무효과다.
- 5개 시험 PASS(0.04초), 그중 1,444개 작은 산술 조합 검사. all-targets clippy PASS. 기존 journal 시험은 이번 독립 모듈 변경에서 재실행하지 않았다.
- 잠정 fee는 fill에 보존한다. 체인 C를 이동하거나 수수료를 확정하지 않는다.

## 남은 구현

실제 서명/ID binding→시퀀서→OrderBook callback 정규화→이 장부→journal/outbox 연결, 전체 연결 성분 epoch 정정과 snapshot 검증, REST 연계용 Rust 서비스, 결정적 재생 및 통합 crash 시험이 남았다. 이 장부의 correct_fill은 전체 epoch 정정 API가 아니며 호출자는 전체 정정을 하나의 후보 상태/commit에 묶어야 한다. 일반 JSON 직렬화와 제품 schema 연결도 아직 없다.

합성 잔고 단위시험이며 실제 예치/체인 연결·제품 S2-AT PASS를 주장하지 않는다. 성능/CPU/RSS 미측정, 추가 유료 자원 없음. 시험시간은 개발 소요가 아니다.

재현 명령·계약/config/lock/소스 hash는 manifest.json, 원시 결과는 tests.txt와 clippy.txt에 있다. 기준 소스 SHA는 ledger 변경 직전 head이며 정확한 변경은 별도 게시한 patch와 후속 commit에 고정한다.
