# NUS-47: fork/exec와 journal writer 잠금 수명

## 결함과 근거

승인 Exchange 기반은 `d0cf18f1294941b19e72102568eb220a7a5854f1`이다.
Settlement head `b630bee4a6f2f28dfee6b22a19cc991705ede83d`의
[원시 CI](https://github.com/nus-gang/cosmo-dex/actions/runs/37122760728/job/111201978269)는
`committed_record_and_outbox_recover_atomically`의 `drop(j)` 다음
`Journal::open`에서 `WriterAlreadyRunning`으로 실패했다(12 PASS / 1 FAIL).

기존 구현은 잠금 File의 close만으로 소유권을 해제했다. 실제 사용
[Rust 1.92.0 Unix 구현](https://github.com/rust-lang/rust/blob/1.92.0/library/std/src/sys/fs/unix.rs)은
`O_CLOEXEC`로 파일을 열고 `flock(LOCK_EX | LOCK_NB)`로 잠근다.
[Linux flock 의미](https://man7.org/linux/man-pages/man2/flock.2.html)에 따르면
fork/dup의 디스크립터는 같은 open file description의 잠금을 공유한다.
명시적 `LOCK_UN` 또는 모든 디스크립터의 close까지 잠금이 남는다.
`CLOEXEC`는 exec 이후를 처리하며 fork부터 exec 사이의 상속을 없애지 않는다.

따라서 다른 시험 스레드가 subprocess를 만드는 동안 원 소유자가 drop되면,
실제 writer는 끝났어도 자식의 상속된 디스크립터가 일시적으로 재개방을 막는다.
시험 경로는 PID + 원자 증가 번호로 구분되므로 동일 디렉터리 이름 충돌이 아니다.
병렬 시험에서 드러나는 **journal 잠금 수명 결함**이며 시험 직렬화로 해결할 일이 아니다.
원시 실행에는 syscall trace가 없어 어느 자식이 당시 디스크립터를 상속했는지는
단정하지 않는다. 아래 결정적 실험은 그 실패를 일으킬 수 있는 실제 구현 결함을 분리한다.

## 결정적 회귀

`dropped_writer_reopens_while_unrelated_child_is_before_exec`는 다음을 검증한다.

1. 합성 record/outbox를 append하여 commit receipt를 보관한다.
2. 다른 스레드의 `Command::pre_exec`에서 자식을 멈춘다. socket handshake로
   fork 이후/exec 이전임을 확인하며 sleep이나 반복 운에 의존하지 않는다.
3. 원 writer가 살아 있는 동안 추가 writer는 거절된다.
4. 원 writer drop 후 자식이 여전히 멈춘 상태에서 즉시 reopen하고, 같은 commit과
   record/outbox를 복구한다.
5. 새 writer가 살아 있는 동안 추가 writer는 거절된다. 기존 자식이 exec/종료하여
   오래된 디스크립터를 닫은 후에도 새 소유자의 잠금이 유지된다.
6. 별도 프로세스의 잠금 경쟁도 거절되고, 새 append 후 재개방하면 두 record가 복구된다.

pre-exec hook은 준비된 socket에 raw read/write만 수행한다. 메모리 할당,
Journal 접근, assertion, Rust mutex를 사용하지 않는다. socket timeout은
10초이며 자식을 release/reap한 뒤 성공 조건을 검사한다. 두 POSIX syscall의
FFI 선언은 Unix 시험 함수 안에만 있으며 의존성과 `Cargo.lock`을 바꾸지 않는다.

수정 전 재현 commit: `8efdb0866e2742f03e6bcf45f1d6db59da9115f8`.
[기존 Linux CI 재현 실행](https://github.com/nus-gang/cosmo-dex/actions/runs/37124288677).
macOS/Rust 1.92.0에서도 동일 시험의 reopen이 `WriterAlreadyRunning`으로 실패했다.

## 최소 수정과 보존 조건

private `WriterLock` guard를 잠금 획득 직후 생성한다. guard의 Drop이 명시적으로
`File::unlock`을 호출하므로 초기화 실패와 정상 Journal drop 모두 동일하게 정리된다.
Journal의 마지막 필드로 두어 WAL이 먼저 닫힌다. 잠금 획득에 실패한 contender는
guard를 만들지 않으므로 기존 소유자의 잠금을 해제하지 않는다.

`WouldBlock`만 `WriterAlreadyRunning`으로 변환하고 실제 OS 오류는 `Error::Io`로
보존한다. writer.lock 삭제, 잠금 우회, 무조건 재시도, 시험 skip/직렬화는 없다.
WAL framing, marker/fsync 순서, ACK 시점, outbox, 정정 예약 및 재생 로직은 유지한다.
Settlement 파일과 공통 protocol, workflow는 수정하지 않았다.
`ops/ci/manifest.json`에는 변경된 Exchange 소스 두 파일의 SHA256만 갱신한다.
이는 S0 source mapping gate를 유지하기 위한 것으로 oracle/expected case는 바꾸지 않는다.

중간 head `c85067061ad8164e6a2715d7090fccc2884633ff`에서 S2 Exchange CI는
통과했지만, 시험용 dev dependency를 추가하면서 lockfile hash 및 S0 source
manifest 불일치가 생긴 것을 전체 PR checks에서 발견했다. 의존성 추가를 제거하고
Exchange 소스 해시만 갱신하여 수정했다. 해당 실패 로그도 NUS-47 artifact에 남긴다.

## 재현 명령

저장소 root, Rust 1.92.0, 기존 `exchange/Cargo.lock`, 합성 데이터만 사용한다.

```sh
cargo test --locked --manifest-path exchange/Cargo.toml --test s2_journal \
  dropped_writer_reopens_while_unrelated_child_is_before_exec -- --exact --nocapture
cargo test --locked --manifest-path exchange/Cargo.toml --test s2_journal
cargo test --locked --manifest-path exchange/Cargo.toml --all-features \
  --test s2_process --test s2_sequencer --test s2_snapshot
cargo fmt --manifest-path exchange/Cargo.toml --check
cargo clippy --locked --manifest-path exchange/Cargo.toml --all-features --all-targets -- -D warnings
```

로컬 수정 후 journal 14/14, process 6/6, sequencer 40/40, snapshot 9/9 및
fmt/clippy가 통과했다. 고정 최종 SHA의 Linux 전체 Exchange 시험/원시 로그/검토 경로는
Paperclip NUS-47의 artifact와 commit/PR work product에 함께 제공한다.
기존 `.github/workflows/s2-exchange.yml`은 Ubuntu 24.04에서 모든 feature/target의
시험, clippy, schema 검사를 수행한다.

## 한계

이 변경은 정상 drop의 잠금 수명을 고친다. `SIGKILL`/`process::exit`는 Drop을
실행하지 않으므로 상속된 디스크립터가 닫힐 때까지 OS 잠금이 남을 수 있으며,
그 경우 두 번째 writer는 계속 거절된다. fork 자식에서 복제된 Journal을 사용하거나
drop하는 모델은 지원하지 않는다. 자식은 exec/_exit하고 새 writer는 독립 open해야 한다.
unlock 자체의 OS 실패 시에도 File close 및 새 writer의 OS 잠금 검사는 유지된다.
분산 fencing, 네트워크 파일시스템, 전원 차단 내구성, 성능 보장을 추가하지 않는다.
