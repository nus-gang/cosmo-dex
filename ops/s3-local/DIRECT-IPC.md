# 직접 TX 검증 IPC (중간 후보)

`chain/app/cmd/nus-s3-local-direct`는 `dev_local_demo` build tag 전용이다.
명령: `nus-s3-local-direct verify --local-demo-profile nus-s3-local-demo --acknowledge-unproven-space`.
인자는 순서까지 고정이며 stdin은 Go `json.Marshal(localdirect.Request)`의 canonical JSON 단일 원문과 EOF다.
raw/owner/public_key/genesis는 padded base64, chain_id는 문자열,
account_number/sequence는 canonical uint64 십진 문자열이다. 필드 순서는 Request 선언 순서다.
총 192 KiB, TX 139264 bytes, 프로세스 전체 3초 상한이다. 부모도 유한 timeout과 kill/reap을 적용해야 한다.
성공은 version=1, exact tx_sha256, owner_bound=true, broadcast=false의 단일 JSON 행이다.
거절은 exit2·stdout0·고정 DIRECT_TX_REJECTED stderr이며 입력 내용을 출력하지 않는다.

이 helper는 stdin의 신뢰 출처나 freshness를 입증하지 않는다. HTTP 인증 owner와 동일 H Account 입력을
부모가 결합하고, 승인 descriptor에 helper binary SHA를 넣어 실행 사본을 고정해야 한다.
Rust bounded child/HTTP 방송 연결은 아직 미완성이다. 검증 성공은 방송·CheckTx·확정·runtime 승인이 아니다.
서비스/RPC/방송0, runtime pin 미발급, DEV NOT_RUN.

## Rust 부모 전송 (2026-10-07)

`runtime/direct_child.rs`의 `verify`는 typed Request를 Go field 순서·base64·uint64 문자열·HTML escaping과 같은 원문으로 만든다.
절대 경로 executable과 두 opt-in argv, 빈 환경, `/` cwd를 사용한다. 부모의 0초 초과/최대3초 deadline,
nonblocking stdin/stdout/stderr, 출력512 bytes 상한, 유한 입력+EOF, exact 성공 행·exit0·stderr0·양쪽 EOF를 요구한다.
오류/중단/timeout/unwind는 child kill/wait로 정리하고 재시도하지 않는다. spawn 자체의 OS scheduling이나
악의적 executable의 descendant sandbox를 보장하지 않는다. 호출자는 검토된 private 실행 사본을 제공해야 한다.

검증: 합성 subprocess4+실제 Go helper1 = Rust5 PASS/0 FAIL. 실제 ML-DSA 서명 fixture를 Go JSON으로
생성하고 Rust 재인코딩 바이트 일치·helper 성공·sequence/owner 변경 거절을 확인했다. fixture 생성 Go TestIPC1도 PASS.
서로 다른 층의 시험수는 합산하지 않는다. 최초 sha2 포맷 컴파일 오류, 100ms child 시작 fixture 경합,
GOMODCACHE 환경 오류는 보정했고 실패 기록을 보존했다.

HTTP/인증 Account 연결·descriptor/private executable 연결은 아직 미완성이다. helper 성공은 owner 입력의 출처나
freshness를 증명하지 않는다. 실제 서비스/RPC/방송0, runtime pin 미발급, DEV NOT_RUN.

## Descriptor에 결합된 private helper 사본

`staged_direct.stage_snapshot`은 worker와 같은 byte-verified capture의 SRE descriptor에서
`bin/nus-s3-local-direct` SHA256을 읽고 실제 artifact를 대조한다. root0700/file0500 임시 사본을
fsync 후 제공하고 scope 종료/오류/interrupt에 사본만 정리한다. `recheck`로 호출 직전 변조를 거절한다.
입력 capture의 승인·의미 검증은 호출자 책임이며 이 API 자체는 실행 허가가 아니다.
같은 uid의 악의적 변경을 격리하는 sandbox가 아니다. child reap까지 scope를 유지해야 한다.

신규 순수시험4 PASS/0 FAIL: 원본 교체, 권한·SHA, 누락/잘못된 digest·symlink/hardlink,
사본 변조, fsync 실패·interrupt 정리. 합성 bytes만 사용했고 executable 실행0이다.
worker lifecycle/argv 및 Rust direct_child 연결은 다음 단계다. 기존 Rust/Go 시험은 이번 재실행하지 않았다.
