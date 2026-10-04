# S2 통합 실행

단일 호스트의 네 검증인, 합성 DEVBASE/DEVQUOTE, 로컬 `LOCAL_FSYNC` PoC다.
정산·분산 복제·자동 failover·실자산·처리량 보장은 포함하지 않는다.
검증된 main의 설치·사용은 [S2 시작 안내](../../docs/s2-quickstart.md), 완료된 독립 main QA와 최초 통합 시험의 구분은 [검증 기록](../../docs/verification.md#s2-main-인수)을 따른다.

## 같은 checkout 빌드와 시험

Go 1.26.5, Rust 1.92.0, Node `.node-version`, 기존 lock을 사용한다.

```sh
(cd chain/app && sh scripts/build.sh)
cargo +1.92.0 build --locked --manifest-path exchange/Cargo.toml --bin exchange-s2
(cd web && npm ci --ignore-scripts && npx playwright-core install chromium && node s2/build.mjs)
python3 ops/s2/shutdown-test.py --output .evidence/s2/shutdown
python3 ops/s1/log-test.py
python3 ops/s1/init-test.py
python3 ops/s1/integration.py --base-port 31556 --home "$PAPERCLIP_RUN_SCRATCH_DIR/s1" --output .evidence/s1
S2_FOUR_VALIDATORS=1 S2_TEST_CHAIN="$PWD/chain/app/bin/nusd" S2_ENGINE_BINARY="$PWD/exchange/target/debug/exchange-s2" node web/s2/browser.test.mjs .evidence/s2/browser
S2_PROCESS_EVIDENCE="$PWD/.evidence/s2/process" cargo +1.92.0 test --locked --manifest-path exchange/Cargo.toml --all-features --test s2_process
python3 ops/s2/manifest.py --output .evidence/s2
```

브라우저 시험은 `PAPERCLIP_RUN_SCRATCH_DIR`(CI에서는 `S2_TEST_SCRATCH`)에
새 home을 만들며 종료 시 자식을 회수한다. 기존 home을 지우거나 재사용하지 않는다.
네 노드가 같은 확정 블록을 보는지 확인하고 실제 브라우저 생성 공개키를 genesis에
넣는다. 개인키는 브라우저 메모리에만 있고 탭 종료 후 복구할 수 없다.
실제 두 자산 예치→지정가/부분 체결→취소/IOC→미정산 출금 보류→직접 출금 정정→
API 재시작→일반 출금과 계정 전환/지연/단절을 검증한다.
`process` 시험은 별도 합성 snapshot에서 실제 kill·writer 거절·commit 경계 crash를
검증한다. 둘을 실제 체인 crash의 모든 조합 또는 분산 RPO/RTO로 해석하지 않는다.

## Paperclip 관리 서비스

개발용 서비스는 `runtime.py serve`를 Paperclip runtime command로 등록해 기동한다.
직접 background 실행하지 않는다. `init`은 서비스 기동이 아니라 새 데이터 준비다.

```sh
# 공개키 파일은 정확히 두 ML-DSA 공개키의 JSON 배열. 생략하면 fixture 계정.
python3 ops/s2/runtime.py init --home .runtime/s2 --user-public-keys users.json
# Paperclip에 등록할 foreground command:
python3 ops/s2/runtime.py serve --home .runtime/s2
# 관리 start/restart 후 확인:
python3 ops/s2/runtime.py health --home .runtime/s2
python3 ops/s1/devnet.py status --home .runtime/s2/chain
python3 ops/s1/devnet.py log --home .runtime/s2/chain --node 0
```

RPC 30557/30567/30577/30587, API 8788, 웹 5173은 loopback이다. 기존 S1의 28657
계열과 분리한다. 포트가 사용 중이면 기존 서비스를 죽이지 말고 자기 관리 서비스를
먼저 확인한다. S2 전체 관리 stop→status stopped 확인이 사용 종료 절차다.
종료는 API/엔진·웹·검증인을 회수한다. runtime은 각 서비스에 별도 프로세스
그룹을 부여하며, 하위 서비스는 세션/그룹을 이탈해 daemonize하면 안 된다.
검증인 네 개에 동시에 TERM을 보내고 총 15초 뒤 무응답 프로세스를 KILL한다.
runtime은 전체 25초 grace 뒤 자신이 생성한 그룹에 KILL을 보내 후손도 정리한다.
강제 종료·비정상 자식 종료·확인/정리 실패는 0이 아닌 종료 코드로 보고하며,
실패를 정상 `clean_stop`으로 기록하지 않는다. PID/포트/lock 해제와 제어 소켓
정리를 확인한 뒤 재기동한다. `shutdown-test.py`는 정상/지연/무응답·supervisor
조기 종료·정리 권한 오류를 유한 모형 프로세스로 검사한다. 실제 체인 시험과 구분한다. 다시 관리 start하면 같은 DB/journal을 열고
확정 cursor를 따라잡은 후 OPEN이 된다. runtime·node별 lock은 중복 supervisor를 거절한다.
손상 journal 또는 hash 불일치를 빈 원장 생성으로 우회하지 않는다.

검증인 일반 로그는 노드당 100MiB×5(활성+이전 4), 네 노드 합계 2,000MiB다.
관리 wrapper의 chain-supervisor/bootstrap/API/web 로그도 각각 같은 상한을 가지므로 전체 일반
로그 상한은 4,000MiB다. 로그 쓰기 실패 시 전체 자식을 종료하고 오류를 노출한다.
DB·consensus WAL·engine journal/outbox·키·genesis·증거는 로그 정리 대상이 아니다.

manifest에는 후보 SHA/tree·contract/config/lock·binary·genesis hash 및 환경·원시
증거 hash를 기록한다. 검증인 비밀키·node key·브라우저 세션은 업로드하지 않는다.
종료 전 필요한 증거를 issue attachment/artifact로 게시한다. run scratch는 임시이며
영속 서비스 home은 `.runtime/s2`에 둔다. 종료된 실행의 재생성 가능한 `target`,
`node_modules`, `dist`, `bin`만 소유권·활성 프로세스 사용 여부를 확인하고 정리한다.
활성 checkout, 원장, 키와 검증 증거를 포괄 삭제하지 않는다.

관측 항목: 네 노드 높이/peer/catching_up/공통 블록 hash, API mode/observation age,
C/R/D/P/A, journal 오류, writer 거절, 로그 용량/개수, 관리 프로세스 종료 코드.
로컬 재시작 관측 시간과 성공 응답 재생 시험은 SLA 또는 AZ 소실 RPO/RTO가 아니다.
