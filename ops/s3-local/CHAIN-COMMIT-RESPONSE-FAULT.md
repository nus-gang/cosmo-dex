# F08 Chain commit→RPC response 경계

L-R의 기본 비활성 fault 준비 산출물이다. 실제 socket/RPC/서비스 실행과 DEV 판정은
승인 runtime pin 이후 L-T에 남는다.

`runtime/chain_commit_response.rs`는 C가 저장한 exact SETTLE Attempt가
`INCLUDED_SUCCESS`, `abci_code=0`, 동일 TX hash인 경우에만 F08에 진입한다. 아직 그
batch의 Receipt가 없고 batch가 COMMITTED/CORRECTED가 아님을 확인해, 응답 유실 전에
Receipt 조회나 자산 효과가 발생하지 않았음을 분리한다.

`chain-commit-response.jsonl`은 private root0700/file0600/no-replace로 만들고 각 행을
file/root fsync한다. canonical 원문에는 attempt/tx/batch/height와 C commit을 결합한다.
reader는 부분 final을 UNKNOWN으로 유지하며 response delivery, Receipt 조회, asset effect,
crash, authenticity, fsync, replay, reusable permit, durable ACK를 인증하지 않는다.

component 시험은 fee0/25 각각에서 response-loss 오류 뒤 원 Attempt를 다시 조회하고,
승인 Receipt를 한 번 저장한 뒤 Apply를 한 번만 수행한다. 같은 home을 두 번 다시 열어
두 번째 asset effect가 없음을 확인한다. 2026-10-07 기존 설치 Rust 1.92.0과 기존
dependency cache만 사용한 `--offline --locked` build가 통과했고, targeted 실제 C/L-D
시험은 1 PASS / 0 FAIL / 189 filtered, read-only Python reader는 4 PASS / 0 FAIL이었다.
첫 실행의 `/var`→`/private/var` canonical root 불일치는 fixture 경로를 실제 canonical
경로로 고친 뒤 같은 시험을 다시 실행했다. 이 시험은 실제 RPC 응답 유실이나 계약상 3회
반복이 아니므로 F08/DEV09는 NOT_RUN이다.
