# Wallet 정상 서명과 실제 private Go helper 연결

승인 Wallet LocalClient가 일회성 ML-DSA 키로 생성한 출금 TxRaw를 Rust 방송 adapter→private Go helper→전송 callback까지 연결했다. 제품/경제 API 변경 없이 신규 component 시험만 추가했다.

- 신규 Rust 1 PASS/0 FAIL (6 case): 정상 서명에서 전송 성공/오류 각 callback 1회와 SUBMISSION_UNKNOWN, sequence/공개키/서명/genesis 변조에서 callback 0회. 실제 helper SDK/SIGN_MODE_DIRECT/ML-DSA 검증 및 호출 전후 SHA/권한/inode 검사, 사본 정리 확인.
- 방송 adapter 회귀4 PASS/0 FAIL, bridge1 ignored. 신규 실행의 나머지83 모듈시험 미실행.
- Node fixture 생성1 PASS, 선택적 bridge1 skipped. 계층별 시험 수 합산하지 않는다.
- 최초 CARGO_MANIFEST_DIR 누락 컴파일 실패 후 checkout/exchange로 보정하여 컴파일 PASS. 실패/통과 로그 보존.

인증 세션과 Account 응답은 합성이고 방송은 callback이다. 실제 REST 로그인→정상 TX 결합, 실제 HTTP/RPC/체인 검증은 입증하지 않았다. 기존 실제 인증 malformed TX 거절 근거와 이번 정상 서명 통과 근거를 종단 PASS로 합산하지 않는다. private key 저장0, fixture는 공개키/서명 TX만 포함한다.

재현: build.json argv에 CARGO_MANIFEST_DIR=<checkout>/exchange로 컴파일한다. NUS_BROWSER_BOUNDARY=<없는 파일>로 node --experimental-strip-types --test ops/s3-local/browser-broadcast-boundary.test.ts 실행한다. DIRECT_IPC_BINARY=<offline Go binary>, 동일 NUS_BROWSER_BOUNDARY, PAPERCLIP_RUN_SCRATCH_DIR을 설정하고 tests wallet_signed_tx_private_helper_boundary --ignored --test-threads=1 실행. 기존 설치 toolchain/cache, 추가 설치0.

base eb935f7 위 누적 미커밋 소스 보존. 다음 SRE: 실제 인증 정상 TX 연결·웹 entrypoint·새 fee0/25 home/genesis·chain/web launcher·fault/정리·최종 build/binary/web SHA256·다섯 descriptor manifest·CEO/CTO 독립 승인·CTO→Security 심사. 현재 runtime pin 미발급, DEV NOT_RUN. 서비스/START/RPC/방송0·€0. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false와 부모 blocker 유지.
