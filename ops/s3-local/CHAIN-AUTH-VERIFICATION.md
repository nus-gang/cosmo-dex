# ChainPort 실제 인증 세션 연결 검증

기준 eb935f7 위 누적 SRE 소스. 이번 변경은 chain_http.rs의 실제 Rest/Engine 인증 회귀시험이며 제품 인증·경제 로직 변경0이다.

공개 합성 ML-DSA 키로 실제 Rest의 challenge/session을 통과한 두 사용자 세션을 account_from에 연결했다. fee0/25 각각 세션 owner bytes만 Chain query callback에 전달되는지 확인했다. 로그아웃·중복 Authorization·외부 peer·잘못된 origin·5초 초과 stale에서 query0, 정상 query1, Engine commit 불변·종료 후 각각 두 번 replay를 확인했다. 정상 사용자0의 로그아웃 뒤 사용자1이 계속 조회 가능하다.

Rust 컴파일 PASS, 신규1+기존 adapter5 = 6 PASS/0 FAIL. 나머지 포함 모듈62는 이번 미실행. 인증과 C store는 실제 구현이며 Chain query callback은 합성이다. 실제 HTTP socket/Chain RPC/서비스/관리 start-stop0. 실제 browser 로그인이나 DEV 통합 PASS가 아니다.

초기 시험의 stale 기준을 3초로 잘못 가정한 실패와, 미래 관측 뒤 과거 인증 시각을 넣어 세션이 무효화된 fixture 실패를 보존했다. 승인 구현의 5초 초과 기준·인증 시계 단조 조건에 맞게 시험을 보정했다. 구현의 인증 기준은 변경하지 않았다.

방송 owner 결합/결과 HTTP·브라우저 ChainPort·새 home/genesis·chain/web/fault/정리·최종 manifest·독립 승인은 남아 있다. SRE가 이어서 구현한다. CTO→Security 제출 전, runtime pin 미발급·DEV NOT_RUN·€0. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED와 부모 blocker 유지.
