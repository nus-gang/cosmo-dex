# ChainPort 계정 owner 표현 결합 수정

기존 SRE adapter가 REST의 base64 owner를 hex로 해석하고 ChainRead의 Bech32 owner와 문자열 비교하던 결함을 수정했다. 실제 정상 계정 조회를 항상 거절할 수 있는 연결 결함이며 승인 C/L-D 경제/인증 코드는 변경하지 않았다.

REST owner는 C schema::bytes로 canonical base64를 검증하고 20 bytes를 요구한다. Chain 응답은 기존 codec::decode_address로 canonical nus Bech32를 검증한 후 같은 주소 bytes인지 대조한다. 인증 실패/Context/fresh/gate 검사와 peer·중복 header 보존은 그대로다. 공개 ChainPort owner는 브라우저 계약의 Bech32를 유지한다.

검증: 실제 Snapshot fixture → SDK Account 응답 decoder → direct_account_from → HTTP account adapter 회귀를 추가했다. 인증 callback/IO는 합성이며 실제 로그인/RPC는 미실행이다. hex/Bech32/비정규 base64를 REST owner로 주면 조회 전 거절한다. 다른 canonical Bech32 계정·대문자 표현·다른 Context/IO도 거절한다.

Rust 컴파일 PASS, 신규1+기존66 = 67 PASS/0 FAIL. 최종 다른 계정/대문자 assertion 보강 후 adapter5 재시험 PASS, 중복 합산0. 처음 test helper 가시성 compile 오류는 보정하고 로그를 보존했다. 원 계정 adapter의 합성 성공 fixture도 실제 표현으로 고쳤다. 실제 서비스/Chain RPC/관리 start-stop0·runtime pin 미발급·DEV NOT_RUN·€0.

base eb935f7 위 누적 SRE 미커밋 ops/s3-local 원문을 포함한다. 다음 SRE 실행: 방송 owner 결합/결과 HTTP·브라우저 ChainPort·새 home/genesis·chain/web/fault/정리·최종 manifest·독립 승인. CTO→Security 제출 전이며 원 G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED와 부모 blocker를 유지한다.
