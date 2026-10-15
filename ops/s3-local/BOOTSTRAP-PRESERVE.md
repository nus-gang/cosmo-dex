# 초기화 RPC 원문 보존 연결

`runtime/bootstrap.rs::create_preserved`는 상속 RPC 크기 한도를 검사하고 기존 private canonical directory를 fd로 연다. root UID/0700 확인 뒤 `bootstrap-rpc.json`을 openat/O_EXCL/O_NOFOLLOW/0600으로 새로 쓰고 file fsync→directory fsync→root inode 재대조를 끝낸 뒤 기존 C create를 호출한다. 기존 파일/링크를 대체하지 않으며 실패 원문과 부분 파일은 보존한다. 같은 경로 자동 재시도/수리/삭제 없음.

신규2+기존14 = 16 PASS/0 FAIL/SKIP0. fee0/25 성공/의미 거절의 exact RPC 보존, 0600, 기존 증거 재게시 거절, 두 번 C replay, collision/link/public parent/oversize의 home 생성0 확인. fsync 오류 주입과 동시 경로 교체 시험은 이번에 수행하지 않았다. 합성 RPC/descriptor/pin을 사용하며 실제 fetch/서비스0. API 자체가 조직 승인·Paperclip 업로드를 수행하지 않는다. 초기화 CLI는 승인 확인→fetch→이 보존/create 순서를 연결해야 하며 이는 남은 작업이다.

기존 rlib와 Rust1.92.0으로 컴파일. 명령/원시 로그/SHA256 첨부. C/L-D 경제 로직 변경0. 다음 SRE 작업: 초기화 승인/CLI·chain 관리 launcher/fault/정리·최종 build/다섯 descriptor manifest·독립 승인·CTO→Security. runtime pin 미발급·DEV NOT_RUN·G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false·€0.
