# S1 실제 RPC REST 후보

> 현재 제품 기준 main `32781aa97d62ec747e7a25c10fdb8b58030d79f2`에서 실제 브라우저 통합과 S1 인수가 완료됐다. 아래 후보 SHA·미완료 목록은 개발 당시 기록이다. 현재 [사용자 시작 안내](../../docs/quickstart.md)와 [인수 근거](../../docs/verification.md)를 확인한다.

[NUS-21](/NUS/issues/NUS-21). A 계약 d41be80fc325cf76b47b35b3f9b37359544e363e,
실제 Chain 바이너리 ab3e8a8eebe47439a8102b30367396faa291e296를 소비한다.
S0 `settlement/v1`을 변경하지 않는다. Python 표준 라이브러리만 사용한다.

```sh
python3 settlement/s1/server.py --rpc http://127.0.0.1:26657 \
  --genesis-hash <실제_genesis_SHA256> --journal <지속_디렉터리>/txs.sqlite \
  --port 8787 --origin http://127.0.0.1:5173
```

`--rpc`는 신뢰하는 로컬 개발망 CometBFT RPC다. light client 검증기가 아니며
원격 비신뢰 RPC를 권위 원장으로 승인하지 않는다. 서버는 127.0.0.1에만 bind한다.
Wallet의 정확한 origin을 명시한다. 개인키/seed/새 sequence 입력은 받지 않는다.
서명자는 Wallet/CLI에 남으며 REST는 서명 bytes를 변경하지 않는다.

| 경로 | 동작 |
|---|---|
| GET /s1/network | chain/genesis, 계약·앱 버전, 자산·정밀도, committed 높이 |
| GET /s1/accounts/{owner} | 계정/sequence/epoch/DEVQUOTE 은행·확정 채권/DEVGAS |
| POST /s1/txs | `{ "tx_bytes": "canonical padded base64" }`만 수신, 202 UNKNOWN |
| GET /s1/txs/{uppercase SHA256} | index의 TX bytes/hash, 실제 block 포함 위치와 block_results를 대조 |
| GET /s1/accounts/{owner}/requests/{lowercase hex64} | snapshot과 같은 높이의 immutable receipt |

모든 정수는 십진 문자열이다. POST의 `check_tx_code=null`/`observed_height=null`은
응답 유실 시 미관측을 뜻한다. CheckTx 0/HTTP 202는 확정이 아니다.
조회 상태 COMMITTED는 블록 포함+실행 code 0, REJECTED_FINAL은 포함+실행 code 비0이다.
TX index off/lag/미존재는 UNKNOWN이며 receipt를 별도로 조회할 수 있다.
404 NOT_FOUND_AT_HEIGHT는 해당 높이의 부재이며 확정 실패가 아니다.

잔고 캐시/배경 인덱서가 없다. 매번 같은 committed snapshot에서 DEVQUOTE 및 DEVGAS
보존식을 검사한다. `cursor_height=observed_height`, `indexer_mode=DIRECT_COMMITTED_QUERY`.
`block_time`, `freshness_ms`(로컬 시계 기준), `query_latency_ms`로 관측 신선도를 노출한다.
높이/chain/genesis 불일치·보존식 실패는 503이며 잠정 금액을 잔고에 합치지 않는다.
정지된 체인의 과거 COMMITTED 잔고는 신선도가 증가하며 새로운 확정을 뜻하지 않는다.

SQLite journal은 송신 **전에** TX hash와 원문 bytes를 저장한다. 재시작 시 genesis 결합을
확인한다. 저장 실패 시 송신하지 않는다. 동일 bytes 중복 제출은 같은 hash로 저장되며
새 sequence 재서명은 하지 않는다. journal 삭제 후에도 잔고/receipt/TX 조회는 체인에서
복원된다. journal 유실 시 로컬 제출 목록을 복원한다고 주장하지 않는다. Wallet은 원래
signed bytes/hash/request ID를 보존하고 불확실 상태에서 새 ID·sequence를 자동 생성하지 않는다.

## 검증

```sh
python3 -m unittest discover -s settlement/s1 -v
python3 settlement/s1/smoke.py --binary <nusd 경로> \
  --operators <chain/app/config/operator-accounts.json 경로> --output <새_시험_디렉터리>
```

smoke는 29756/29757 포트의 임시 단일 노드와 임의 포트 REST를 구동하며 finally에서 종료한다.
지속 preview/runtime이 아니다. 두 사용자 각각 100 DEVQUOTE 예치/40 출금/60 확정 채권,
같은 TxRaw 재전송, 체인·REST 재시작, 빈 journal 재구축을 실제 HTTP로 검증한다.
`evidence/single-node.json`은 binary SHA/genesis SHA/TX hash/높이/receipt를 기록한다.
단위시험의 RPC doubles는 응답 유실·index 누락·높이/보존 불일치 경계를 검증하며 실제 체인
성공 근거로 사용하지 않는다.

## 4검증인 REST 검증

```sh
# 이미 실행 중인 관리 개발망에 연결한다. 노드 stop/reset은 하지 않는다.
python3 settlement/s1/four_validator.py --managed --home <C_runtime_home> --output <새_증거_디렉터리>
# CI: 독립적인 4검증인 자식 프로세스를 만들고 finally에서 모두 종료한다.
python3 settlement/s1/four_validator.py --home .runtime/rest-ci --output .evidence/rest-ci
```

C 후보 `6f869c1a10bd28b5d5bba0990151df5fb39c4b3d`의 관리 RPC
`http://127.0.0.1:28657`, genesis
`61b147973985b60dec19545d049706f1dfc08cf28b35a43b12b70dc6e4b406d2`에 대해 PASS.
공개 fixture 두 사용자의 실제 서명 TX/HTTP 제출·출금, 실제 broadcast 응답 유실 후
원래 hash 복구, 동일 bytes 중복 제출 시 원장/sequence 불변, REST 재시작 및 빈
journal의 잔고·영수증·TX 복원을 검증했다. 사용자 번호는 snapshot 배열 순서가 아닌
실제 genesis 공개키로 매핑한다. 공유 개발망의 초기 잔고 대신 전후 증감을 대사한다.
시험 첫 실행은 이 순서 가정 때문에 영수증 조회 404로 실패했으며 수정 후 통과했다.

`S1 REST four-validator integration`은 push/PR마다 항상 실행한다.
S0 manifest에서 정확히 `settlement/s1`만 전용 CI 경계로 제외하며, S0 원본·oracle·coverage
해시는 유지한다. 경계 시험은 S0 변경/삭제와 유사 경로 추가를 계속 거부함을 확인한다.

미완료:
[NUS-22](/NUS/issues/NUS-22) 실제 브라우저 통합, CTO→Security 독립 검토, main CI/독립 QA.
현재 산출물은 미병합 후보이며 AT01/05/06/07 또는 S1 전체 완료가 아니다.
