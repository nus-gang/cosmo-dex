# S1 로컬 4검증인 개발망

실제 `chain/app`의 nusd(BaseApp/CometBFT) 4개를 같은 호스트에서 구동한다.
테스트 사용자 2명, DEVQUOTE 하나와 별도 DEVGAS만 사용한다.
노드 중단 시험은 호스트/랙/지역 분산, 분산 WAL 복제, 운영 키 격리,
독립 비상 회수 또는 운영 SLA 검증이 아니다.

## 준비와 사용자 실행 (Paperclip 토큰 불필요)

Go 1.26.5, Python 3.10 이상, macOS/Linux를 사용한다. 저장소 root에서:

```sh
(cd chain/app && sh scripts/build.sh)
python3 ops/s1/devnet.py init
python3 ops/s1/devnet.py serve
```

`serve`는 foreground supervisor다. 별도 터미널에서:

```sh
python3 ops/s1/devnet.py health
python3 ops/s1/devnet.py status
python3 ops/s1/devnet.py log --node 0
python3 ops/s1/devnet.py stop --node 3
python3 ops/s1/devnet.py start --node 3
python3 ops/s1/devnet.py restart --node 2
python3 ops/s1/devnet.py stop
python3 ops/s1/devnet.py start
```

전체 종료는 `serve` 터미널의 Ctrl-C다. supervisor가 자식 프로세스를 종료·회수한다.
`stop`은 validator만 멈추고 supervisor를 유지한다. 정상 stop은 SIGTERM을 보내고
15초 뒤에도 종료하지 않으면 SIGKILL한다. 재기동은 기존 DB/서명 상태를 사용한다.
예상하지 못한 노드 종료는 supervisor 전체 실패로 보고하고 나머지도 정리한다.
자동 재시작으로 장애를 숨기지 않는다. 설정·포트 충돌은 노드별 로그를 확인한다.

기본 home은 `.runtime/s1`이다. `--home`은 모든 명령에서 같은 경로를 지정한다.
`init --base-port 28656`의 포트 배치는 다음과 같다.

| 노드 | P2P | RPC | voting power |
| --- | --- | --- | --- |
| 0 | 28656 | 28657 | 10 |
| 1 | 28666 | 28667 | 10 |
| 2 | 28676 | 28677 | 10 |
| 3 | 28686 | 28687 | 10 |

RPC/P2P는 127.0.0.1만 사용한다. 동일 호스트의 peer 연결을 위해
`allow_duplicate_ip=true`, `addr_book_strict=false`, `pex=false`를 명시한다.
각 home의 합의 키/노드 키/DB/서명 상태는 별개다. home은 0700, 키와 제어 Unix
socket은 0600이다. 같은 OS 계정/호스트라는 신뢰 경계는 공유한다.
사용자 거래 키는 CLI의 공개 합성 fixture이며 실자산을 보내면 안 된다.
운영 배분 4주소는 서명 키가 없는 모의 주소이며 비상 가스 지갑이 아니다.

`init`은 각 노드에서 임의 개발 합의 키를 생성한 뒤 동일한 genesis bytes를 배포한다.
따라서 새 `init`마다 genesis hash는 다르다. 기존 home을 덮어쓰지 않는다.
재현 기준은 `.runtime/s1/manifest.json`의 정확한 genesis/config/binary/go.mod/go.sum
hash 및 `version.execution_sha`다. 키는 첨부/CI artifact에 포함하지 않는다.
기존 home에서 binary/genesis/config 변경을 발견하면 실행을 거절한다.
업데이트 시험은 새 home·새 포트를 사용하고 이전 원장을 별도로 보존한다.

## 실제 거래

새 fixture 개발망의 거래소 잔고가 0인 계정에서 한 번 실행한다.
예치 확정을 확인한 뒤 출금하고, 출금 확정 뒤 snapshot을 다시 조회한다.

```sh
chain/app/bin/nusd snapshot --rpc http://127.0.0.1:28657
chain/app/bin/nusd tx --rpc http://127.0.0.1:28657 --user 0 --op deposit --amount 100000000 --request-id 0000000000000000000000000000000000000000000000000000000000000001
chain/app/bin/nusd receipt --rpc http://127.0.0.1:28657 --user 0 --request-id 0000000000000000000000000000000000000000000000000000000000000001
# 예치 확정을 확인한 뒤 다음 출금을 실행한다.
chain/app/bin/nusd tx --rpc http://127.0.0.1:28657 --user 0 --op withdraw --amount 40000000 --request-id 0000000000000000000000000000000000000000000000000000000000000002
chain/app/bin/nusd receipt --rpc http://127.0.0.1:28657 --user 0 --request-id 0000000000000000000000000000000000000000000000000000000000000002
```

두 번째 사용자는 위 명령의 `--user 0`을 모두 `--user 1`로 바꿔 실행한다.
금액은 정규 십진 정수 atoms다. [S1 계약](../../protocol/s1/CONTRACT.md)의
`decimals=6`에 따라 1 DEVQUOTE=1,000,000 atoms이므로,
100 DEVQUOTE=100,000,000 atoms, 40=40,000,000, 확정 거래소 잔고 60=60,000,000이다.
출금 확정 후 `snapshot`을 다시 실행해 해당 계정의 `exchange_atoms`가
`60000000`인지 확인한다. [브라우저 시연](../../web/s1/README.md#입출금-시연-절차)도 같은 금액이다.
수수료는 별도 DEVGAS다. CheckTx 성공/HTTP 200을 확정으로 취급하지 않는다.
확정은 실제 inclusion height>0, check_tx.code=0, tx_result.code=0과 영수증으로
확인한다. 응답이 유실되면 기존 request ID/TxRaw/hash로 조회한다. 결과 확인 전
새 sequence/request ID로 재서명하지 않는다. 중복/초과출금 등의 앱 경계는
`chain/app`의 SDK 회귀에서, 4검증인 합의는 아래 통합 시험에서 검사한다.

## CI 및 장애 재현

CI의 정확한 job 이름은 `S1 four-validator integration`이다. push와 pull_request에
경로 필터/조건 없이 실행된다. S0의 `Go Rust TS component tests`,
`rc4 oracle and E regression`, `pinned-review`와 S1 앱 `chain`을 보존한다.
필수 check 등록은 해당 head에서 각 이름의 성공을 확인한 뒤 수행한다.
CI 결과·main 보호 변경 결과는 Paperclip 인수 보고서에서 확인한다.

```sh
# 새 home을 사용하는 유한 통합 시험; 종료 시 모든 자식 프로세스 회수
python3 ops/s1/integration.py --home .runtime/s1-test --output .evidence/s1-test --base-port 29656
# 이미 실행 중인 관리 runtime에 연결하는 시험 (새롭고 거래 전인 home 필요)
python3 ops/s1/integration.py --managed --home .runtime/s1 --output .evidence/s1-managed
```

`integration.py`의 합성 입력은 사용자 시연과 별개로 유지한다. 각 사용자에게
1,000,000 atoms(1 DEVQUOTE)를 예치하고 400,000 atoms(0.4 DEVQUOTE)를 출금해
600,000 atoms(0.6 DEVQUOTE)의 확정 거래소 잔고를 검사한다.
작은 금액 입력으로도 원장 보존식과 합의·복구 검사는 유효하며, 이 문서 정정은
기존 시험의 입력·판정이나 자산 소수점 규약을 변경하지 않는다.

시험 순서: 4개 서로 다른 검증인·동등 power·공통 genesis → 같은 높이 block ID/app hash
및 3개 이상 commit 서명 → 두 사용자 입출금/보존식 → 1개 정지 후 실제 TX 확정 →
2개 정지, 5초 drain 후 15회(약 15초) 높이 고정·원장 불변 → 정지 중 CheckTx로
접수한 원래 서명 TX가 복귀 후 확정 → 네 노드 동일 높이 hash → 전체 stop/start 후
원장·영수증 동일성. 합의 정족수는 투표권의 2/3 초과이므로 4개 중 3개가 필요하다.
`--managed` 시험은 성공/실패 후 전체 start를 시도해 정지시킨 노드를 복구한다.
기존 사용자 거래가 있는 home에는 이 신선 원장 시험을 실행하지 않는다.

`report.json`은 PASS/FAIL·실행 SHA·hash·중단/복귀 관측값을 기록한다.
`.evidence/s1-test`에는 공개 genesis/config, TX/영수증, 같은 높이 commit, 로그가 있다.
원격 artifact는 GitHub Actions의 `s1-four-validator-evidence`에서 내려받는다.
개발 키/DB는 artifact에 포함되지 않는다. 실패 시 evidence와 validator 로그를 읽고
실제 TX 검증이 완료되지 않았음을 유지한다. 기존 T01~T16 정의는 바꾸지 않는다.

## Paperclip 관리 실행

Paperclip에서는 로컬 `serve &`로 우회하지 않고 이 업무의 execution workspace
runtime에 아래 `workspaceRuntime`을 등록하고 관리 `start/stop/restart`를 사용한다.
서비스 cwd는 이 branch checkout이다. init/build를 완료한 뒤 start한다.

```json
{
  "services": [{
    "name": "nus-s1-four-validators",
    "command": "python3 ops/s1/devnet.py serve",
    "cwd": ".",
    "port": 28657,
    "lifecycle": "shared",
    "reuseScope": "execution_workspace",
    "readiness": {"type": "http", "timeoutSec": 60}
  }]
}
```

HTTP readiness는 포트 기동 증거일 뿐, 확정/정상 합의 증거는 `health`와 통합 시험이다.
관리 runtime이 할당한 runtimeServiceId/실제 URL을 `runtime_service` work product에
기록한다. 등록/기동에 실패했다면 실행 중이라고 표시하지 않는다. 관리 런타임 명령
설정에는 host-command 권한이 필요하며 현재 agent API key만으로 변경할 수 없다.
검증인 자식 프로세스에는 PATH/HOME/TMPDIR/LANG만 전달해 Paperclip/GitHub 토큰을
전달하지 않는다.

## 관측·복구와 한계

관측 항목은 노드별 프로세스 종료, peer 수, latest height/진행 지연, catching_up,
같은 높이 app hash/block ID, 합의 commit 서명 수, TX inclusion/result/receipt,
DEVQUOTE 지갑 합+module=genesis 공급 및 module=확정 거래소 잔고 합,
별도 DEVGAS 사용자+운영+collector 공급 대사다. latest app hash는 서로 다른 높이끼리
비교하지 않는다. RPC 장애 시 살아 있는 노드의 RPC를 같은 genesis와 대조하여 사용한다.
정족수가 없으면 다른 RPC 사용만으로 확정이나 출금을 복구할 수 없다.

백업은 네 노드 전체 stop 뒤 각 home/config와 data를 함께, 접근 제한된 저장소에
보관한다. 복원은 원래 프로세스가 중지됐음을 먼저 확인한 뒤 같은 binary/genesis로
진행한다. 오래된 validator signing state를 되돌리거나 동일 키 복제본을 동시에
실행하면 안 된다. 새 genesis에 기존 DB를 붙이지 않는다. 이 시험은 디스크 유실·
백업 복원 시험이 아니며, 전체 호스트 유실의 RPO/RTO는 미측정이다.
프로세스 중단/복귀에서 관측한 지연은 해당 호스트의 실험값일 뿐 서비스 보장이 아니다.
장기 200,000 engine ops/s·3,000 finalized TX/s도 이번 시험의 성능 보장이 아니다.

## 지갑 공개키로 새 개발망 초기화

[공개키 입력 규약](../../chain/app/USER-PUBLIC-KEYS.md)을 따른다. 지갑에서 내보낸
정확히 두 canonical standard base64 ML-DSA-65 공개키(각 1952 bytes)의 JSON 배열을
사용한다. 객체 wrapper·개인키·seed는 전달하지 않는다.

```sh
python3 ops/s1/devnet.py init --home .runtime/s1-wallet --user-public-keys users.json
python3 ops/s1/devnet.py serve --home .runtime/s1-wallet
```

Paperclip에서는 위 serve를 직접 시작하지 않고 해당 home을 지정한 관리 runtime 설정을
사용한다. init은 네 노드의 사용자 공개키와 운영자 배분을 포함한 최종 genesis bytes를
동기화하고 manifest의 새 genesis hash를 pin한다. 기존 home은 거부한다.

재설정 순서: 지갑 제출과 관리 runtime 정지 → 기존 home 보존 → 새 빈 home에 공개키로
init → 네 genesis hash 일치 확인 → 새 home으로 관리 runtime 설정/기동 →
이전 TX/sequence/epoch/receipt 캐시 폐기 → snapshot에서 새 주소와 genesis 확인.
새 request_id로 지갑이 DIRECT 서명한 TX를 기존 broadcast 경로로 제출한다.
사용자가 거래한 기존 원장을 이 과정에서 재사용하지 않는다.

공개키 옵션을 생략하면 fixture 동작을 보존한다. 위 실제 거래 예시의
`tx --user`, `receipt --user`와 현재 `integration.py`는 fixture 전용이다.
사용자 공개키 개발망에 fixture 통합 시험을 실행하지 않는다.
공개키 초기화 검증과 실제 사용자 지갑 TX/4검증인 합의 검증은 별도 증거로 기록한다.

## 검증인 로그 보관 제한

`serve`는 각 검증인의 stdout와 stderr를 같은 pipe에서 수집하며 오류 줄도
필터링 없이 기록한다. 기본값은 파일당 **100MiB (104857600 bytes)**,
노드당 **활성 `nodeN.log` + 이전 `.1`~`.4` 총 5개**다. `.1`이 가장 최근이다.
4개 노드의 이 일반 로그 합계는 **2,000MiB** 이하이며 다음 쓰기 전에
회전한다. 개행 없는 큰 출력도 64KiB 이하 청크로 수집하고 경계에서 분할한다.
분할된 오류 메시지는 이전 파일과 활성 파일을 순서대로 연결해 읽는다.
오래된 오류도 5개 파일 보관 범위를 벗어나면 삭제되므로 영구 감사 저장소는 아니다.
DB·consensus WAL·키·genesis·validator signing state와 별도 보존한 사고 증거는
이 제한의 대상이 아니며 수정하거나 삭제하지 않는다.

크기 초과 기존 로그는 자동으로 자르지 않고 기동을 거부한다. 운영자가 증거를
보존하고 해당 로그만 정리한 뒤 재기동해야 한다. 권한/공간/rename/쓰기 실패는
supervisor 오류로 전파하며 모든 검증인을 종료하고 0이 아닌 코드로 종료한다.
실패 진단은 Paperclip이 수집하는 supervisor stderr에 남는다. 디스크가 꽉 찼다면
마지막 출력의 파일 저장은 보장하지 않는다. 실패를 무시한 계속 실행이나 자동 재시도는
하지 않는다. 정상 종료는 자식 프로세스 종료 후 pipe를 비우며, 매 쓰기는 사용자 공간
버퍼 없이 기록한다. 전원 장애까지 보장하는 fsync 감사 로그는 아니다.

Paperclip에서는 기존 관리 stop → 변경 적용 → 관리 start 순서를 사용한다.
동일 home과 pinned binary/genesis/config를 유지하고, `/status` 높이 증가와
`nusd snapshot`의 observed_height를 제외한 원장을 비교한다. manifest 재생성,
`init`, DB/WAL/key 삭제로 재기동 문제를 우회하지 않는다. `log` 명령은 최근
64KiB 내 최대 100줄만 읽는다. S2 관리망도 이 supervisor의 같은 기본값을 재사용한다.

검증: `python3 ops/s1/log-test.py`는 실제 100MiB 경계 회전, 5개 보관 및 재오픈,
stdout/stderr·개행 없는 출력, 부분 쓰기, 공간 부족, rename 실패, 기존 증거 보존,
쓰기 실패 시 실제 자식 프로세스 전체 종료를 시험한다. 항상 실행하는
`S1 four-validator integration` CI job에 포함된다.
