# S2-C 서비스 구현·검증 인계

2026-10-03 · Exchange · PR #27의 구성요소 검토 후보. 공통 protocol 변경 없음.
전체 S2 제품 PASS, main 병합, 실제 체인 REST/브라우저 인수는 별도 배정 범위다.

## 완료한 경계

- 실제 ML-DSA Order/Cancel/WalletChallenge, 한 번만 소비하는 nonce와 owner/Origin 세션, 개인 조회 격리.
- 단일 writer 프로세스 `exchange-s2`, 부모 관리 JSON-lines pipe, 공개/개인 경로·서명 body 검증. HTTP/RPC 수집은 NUS-39의 adapter 경계다.
- 단일 sequencer의 GTC/IOC/FIFO·부분 체결·잔량 취소·STP·만료·ID binding, C/R/D/P와 잠정 수수료, 보수적 epoch 의존 정정·출금 freeze/abort.
- 전체 상태·결과·원문·서명·held outbox를 한 journal에 저장하고 fsync/marker 뒤 공개. 전체 semantic replay와 원 receipt 인덱스, bootstrap/context/hash 대조.
- 현재 후보의 전체 signed 이력을 포함한 최대 정정 인코딩 길이 계산, 16MiB 상한과 독립 reserve. commit 도중 재확보 실패는 UNKNOWN으로 닫힘.
- Status 응답 revision은 영속 예약하며 LedgerView revision은 계약대로 command_seq다. opaque cursor는 75문자의 owner/context/seq 결합값으로 Text schema를 만족한다.

## 검증

`tests.log`의 all-features/all-targets Rust 테스트 **104개 PASS**, 실패/ignore 0이다.
13개 journal 테스트 중 child helper 1개를 포함하며 그 helper의 별도 프로세스 실행을 중복 합산하지 않았다.
시험 뒤 `/usr/bin/time -l`의 `kern.clockrate` 접근만 sandbox에서 거절되어 wrapper exit 1이었다.
Cargo 테스트 출력은 마지막 suite까지 모두 PASS이며 자원 측정 실패를 테스트 실패/성능 보장으로 바꾸지 않았다.
마지막 pagination 보완은 `capacity-tests.log`/`capacity-timing.json`의 cargo exit 0으로 추가 검증했다.

- 기존 S0 Rust 암호/codec/policy와 upstream IOC 회귀 포함.
- `s2_runtime`: 인증 domain/nonce/TTL/Origin, 세션 교체/로그아웃/시계 역행/재시작, receipt 201/200, owner 격리, 정정/원 receipt 보존.
- `s2_process`: signed append와 correction 각각 6곳 강제 종료, ACK 뒤 kill/restart, 응답 유실·재시도, 두 번째 writer 거절, 손상 7종의 fail-closed·원본 보존. commit 전 공개 효과 없음, 성공 ACK prefix 무음 소실 없음.
- 정정 append 및 reserve 재확보의 errno 28 오류 **주입**: UNKNOWN/후속 접수 차단, 재시작 시 이전 상태 또는 완전한 정정 복원. 호스트 디스크 전체 ENOSPC 실험은 아니다.
- 외부 ACK ledger와 WAL+marker 동시 rollback 불일치 탐지. 로컬 파일만으로 rollback을 탐지하지 못하는 한계를 명시했다.
- 실제 서명 후보 주문 1,002개/체결 1,001개: 정정 payload **12,023,334B**, 보수적 상한 **12,898,263B**, 한도 **16,777,216B**. 전체 ID·서명·이력을 유지하고 개인 fill 페이지는 1000+1로 조회. 이 대규모 시험은 후보 직렬화/접수 사전 검사이며 1,002개 디스크 명령 처리량 실험이 아니다.
- clippy all-targets/all-features `-D warnings`, fmt, schema 5개, S0 계약, S2 명세 5,360건, S0 manifest 경계 5개 PASS.
- 승인 Chain의 저장된 Comet 예치 snapshot과 실제 genesis bytes hash를 대조해 Rust 서비스 기동 호환성을 확인. 새 RPC/예치 인수는 수행하지 않았다.

명령은 `exchange/S2.md` 및 manifest/로그를 따른다. macOS arm64/Rust 1.92.0,
기존 Cargo.lock 고정이며 새 라이브러리 버전을 추가하거나 기존 버전을 바꾸지 않았다.
S0 oracle port lock에는 기존 engine lock의 동일 version/checksum 의존 그래프를 반영했다.
S0 manifest는 기존 생성기로 현 소스 hash만 갱신했으며 oracle/기대값/검사 요구를 완화하지 않았다.
별도 `s2-exchange` CI에서 Linux의 전체 시험·fault 시험·clippy·schema를 재현한다.

## 수정 중 확인한 실패

- 중단된 코드의 LedgerView/Status revision 혼동을 고정 계약에 맞춰 수정하고 시험도 각각 검사한다.
- 과거 원격 CI의 Paperclip 전용 env unwrap 7개를 일반 환경에서도 실행되게 수정했다.
- S0 manifest/Cargo.toml hash drift와 oracle port의 runtime 의존 graph 누락을 수정했다.
- 내부 unit enum이 extra field를 무시하던 경계를 빈 struct variant로 바꾸어 strict JSON 시험을 통과했다.
- auth/session fixture의 임시 contract/config hash를 실제 승인 pin으로 교체했다. 공식 fixture 원본은 보존했다.
- clippy의 불필요한 비교용 String 생성 지적을 수정했다.

## 인계·한계

NUS-39: 승인 Chain genesis/manifest 고정, H별 RPC/header 대조·연속 observe/rpc_failed,
loopback HTTP/CORS/timeout·원시 body 제한, private 캐시/계정 generation, UNKNOWN 조회·재시도.
NUS-41: 같은 후보의 4검증인/서비스/Wallet 통합과 CI. CTO→Security는 이 PR 고정 head를 심사한다.
main 반영은 I/CEO, 실제 main 독립 QA는 J다. 자세한 wire/command/응답은 `exchange/S2.md`.

LOCAL_FSYNC만 제공하며 분산 ACK/T08/T10·전원 손실/F_FULLFSYNC·실제 디스크 용량 보장·
온체인 정산·WS 전체를 검증했다고 주장하지 않는다. outbox 제출은 항상 비활성이다.
처리량·finalized TX/s 미측정, 추가 유료 비용 0. 테스트 실행시간은 개발 납기 추정으로 쓰지 않는다.
개발/검토/통합 잔여 시간은 전문 심사와 D/F 인수 결과에 따라 다시 추정한다.
