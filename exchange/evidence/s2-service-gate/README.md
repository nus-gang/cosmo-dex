# S2 서비스 접수 gate 체크포인트

Service가 SignedRecovery를 소유하고 재시작 직후 CATCHING_UP으로 시작한다. 신뢰 어댑터의 검증 snapshot 관측/영속 commit 뒤에만 OPEN이 가능하다. RPC 실패·시간 경계·catching_up·높이 공백은 신규 ORDER/WITHDRAW_ABORT를 막는다. gap의 목표 높이를 기억하여 이전 높이 재관측으로 열리지 않는다. 높이 역행/관측 불일치는 현재 프로세스에서 RECOVERY_REQUIRED로 고정한다. 취소/출금 동결은 신규 매칭을 만들지 않는다.

실제 서명 검증과 기존 binding 조회 뒤 gate를 검사하므로 닫힌 상태의 원 요청 재시도는 기존 receipt를 반환한다. 새 요청은 ID/sequence/journal에 효과를 남기지 않는다. 영속 오류는 기존 poison 경계를 유지한다. 신선도는 조회/제출 시각마다 재평가한다. live mode/reason은 기존 journal/receipt의 hash를 고쳐 쓰지 않는 별도 projection이다.

검증: 시퀀서 35개(신규 3) PASS 2.10초, all-targets clippy PASS. 첫 시험에서 신규 order_id fixture를 잘못 넣어 NON_CANONICAL_WIRE 실패 후 32-byte hex로 수정했다. 설치된 Rust 홈을 명시하여 도구 실행 경로 문제도 해소했다. timer 표시는 처리량 측정이 아니다.

제한: 아직 HTTP/API 프로세스가 아니다. RPC fetch/세션 인증·Status schema revision 및 observation 투영 연결, 최대 정정 payload 계산·프로세스 crash 통합이 남았다. 부팅 후 trusted adapter는 체인 tip/catching_up을 다시 조회해야 한다. gap target/관측 실패는 live 상태이며 재시작 후 과거 관측만으로 OPEN을 승인해서는 안 된다. 이 모듈은 RPC consensus 증명을 제공하지 않는다. SignedRecovery는 하위 검증용 API이며 서비스 transport는 Service 경계를 사용해야 한다. 전체 제품 PASS나 CTO→Security 검토 요청이 아니다. 실제 chain 연결은 D/F 인수에 남는다.
