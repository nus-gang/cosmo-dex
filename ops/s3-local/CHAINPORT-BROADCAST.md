# ChainPort 방송 전송 경계

NUS-73 SRE · base eb935f7 위 누적 SRE 미커밋 변경.

ChainRead::direct_broadcast는 canonical base64·상속 139264-byte TX 상한을 IO 전에 검사하고 exact bytes를 broadcast_tx_sync로 한 번만 전달한다. 기존 loopback·2초 총 deadline·bounded HTTP decoder를 재사용한다. 일반 Query allowlist에는 방송을 추가하지 않았다. transport 성공/오류 모두 tx_hash와 SUBMISSION_UNKNOWN만 반환하며 재시도·재서명·CheckTx 기반 확정·엔진 상태 변경이 없다. 결과 확정은 별도 direct_result의 원문 proof에 남긴다.

인증 owner/origin·두 opt-in·새 route/browser 연결은 아직 남아 있다. 이 내부 adapter 자체를 인증 경계나 조직 승인으로 취급하지 않는다. 공개 서비스에 연결하지 않았다. 승인 C/L-D/B 경제·서명 로직 변경0.

검증: 기존 Rust 1.92.0/cache rlib standalone 컴파일 PASS. 신규4+공통 전송 회귀10 = 14 PASS/0 FAIL. exact 최대 TX·빈/과대/비정규 base64 거절·transport 오류별 call1/UNKNOWN·panic 전파/재호출0·query framing/상한 회귀. 나머지44 시험 미실행. 합성 전송 callback/순수 body 시험이며 실제 RPC/listener/서비스0.

다음 SRE 작업: 인증 HTTP/browser ChainPort, 새 fee0/25 home/genesis·chain/web launcher·fault/정리·최종 manifest·독립 CEO/CTO 출처와 CTO→Security 심사. runtime pin 미발급·DEV NOT_RUN·€0. 원 G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED와 부모 blocker 유지.
