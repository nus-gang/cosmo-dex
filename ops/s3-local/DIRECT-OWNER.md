# 직접 TX의 인증 owner 결합 준비

SRE · NUS-73 · 기준 eb935f7 위 누적 미커밋 소스.

chain/app/internal/localdirect의 기본 비활성 dev_local_demo 패키지를 추가했다. B app.Encoding이 제공한 실제 Cosmos SDK v0.55.0 TxDecoder/GetSignBytesAdapter와 ML-DSA VerifySignature를 호출한다. 별도 protobuf/경제 검증 구현은 추가하지 않았다.

단일 MsgDeposit/MsgWithdraw만 허용하며 실제 메시지 owner·SDK signer·공개키 주소가 세션 owner와 같아야 한다. 등록 공개키·genesis·chain ID·account number·sequence를 신뢰 조회 입력에 결합하고 SIGN_MODE_DIRECT 서명을 검증한다. 원 TX bytes를 변경하지 않는다. 최대 TX 139264 bytes를 유지한다. 이 함수의 성공은 freshness·runtime 승인·CheckTx·체인 확정·방송 허가가 아니다. 실제 체인의 경제 검증은 계속 B의 책임이다.

신규 Go 순수시험 2 PASS/0 FAIL. 예치/출금 실제 합성 서명 정상 및 다른 owner/key/genesis/chain/account number/sequence, 절단/과대/서명 변조, MsgSend·메시지 owner 불일치 거절을 확인했다. 2개 시험 내 사례를 독립 시험 수로 합산하지 않았다. 기존 Rust/C 회귀 미실행.

기존 설치 Go1.26.5, GOPROXY=off GOSUMDB=off GOTOOLCHAIN=local, go test -v -count=1 -mod=readonly -tags=dev_local_demo ./internal/localdirect. 새 설치·lock 변경0. 초기 기본 GOPATH 쓰기 거절·local Go1.24.4 버전 불일치·자동 toolchain checksum/기존 cache 쓰기 거절 뒤, 설치 Go1.26.5 직접 경로와 허용된 기존 cache 쓰기로 통과했다.

아직 Rust/HTTP 방송 경로에는 연결하지 않았다. 다음 SRE 실행은 이 순수 검증기의 bounded IPC/인증 HTTP 연결과 결과 조회·browser ChainPort, 새 home/genesis·chain/web launcher·fault/정리·최종 manifest·독립 승인이다. 실제 서비스/Chain RPC/방송0, runtime pin 미발급·DEV NOT_RUN·€0. CTO→Security 제출 전이며 G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED와 부모 blocker 유지.
