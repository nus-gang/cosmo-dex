# S2 공개 호가 조회 체크포인트

Service::book은 committed Candidate 하나에서 BookSnapshot을 만든다. bids 내림차순/asks 오름차순, 가격별 live remaining 합과 주문 수, 전역 seq/revision 및 snapshot 높이를 포함한다. owner/order ID/서명은 공개하지 않는다. BOOK 도메인 canonical hash를 사용하며 RPC 실패나 재시작 자체로 과거 committed 호가 hash를 바꾸지 않는다. 별도 Status를 함께 연결해야 신선도를 알 수 있다. 200 levels/schema 및 산술 overflow는 조용한 잘림 대신 ResourceLimit로 거절한다.

검증: 실제 서명 주문 3건→부분 체결→RPC 실패→journal 재시작 흐름 시험 1개 PASS(0.51초), 고정 계약 fixture schema oracle 3개 PASS, all-targets clippy PASS. 최초 시험의 price 필드명과 buyer fixture ID를 수정 후 재검증했다. 기존 35개 시험은 이번에 재실행하지 않았다. macOS sandbox xcrun 경고가 있었으며 시험은 성공했다.

제한: HTTP/API 프로세스·Status observation/revision·개인 조회/페이지/세션 adapter·최대 정정 용량·프로세스 crash 통합이 남았다. Bid 정렬과 terminal 제거의 별도 시나리오는 후속 서비스 통합에서 보강한다. 실제 체인 인수/제품 PASS/전문 검토 요청이 아니다. 처리량/CPU/RSS는 미측정, 추가 유료 비용 없음.
