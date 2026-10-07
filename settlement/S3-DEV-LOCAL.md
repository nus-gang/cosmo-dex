# L-D 로컬 Settlement component 인계

[NUS-71](/NUS/issues/NUS-71)의 기본 비활성 Rust component다. 승인 C `72b0779474063ce1cd16f4e58417b0422ebd0920`을 포함한다. `exchange` crate의 `dev-local-settlement` feature가 `dev-local-demo`를 명시적으로 포함한다. 기본 feature는 빈 배열이고 표준 실행 파일의 동작을 바꾸지 않는다. HTTP listener·서비스·4검증인·브라우저를 시작하지 않았다. 이 인계는 runtime pin이 아니다.

`G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`, `s3-dev-local/1`, `LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE`, `durable_ack=false`, `UNPROVEN_HOST_SPACE`를 유지한다. 표준 부모의 blocker와 경제·서명·raw/cap·정정 계약은 변경하지 않았다. 새 dependency/lock 변경은 없다.

## 배선 및 실행 인자 계약

L-R이 승인 component를 모아 제공할 launcher는 다음 입력을 요구한다. 이 문서는 아직 존재하지 않는 실행 파일을 기동 명령으로 제시하지 않는다.

- C 기존 `--local-demo-profile <fee0|25 effective JSON 파일>`와 `--acknowledge-unproven-space`, `--runtime-pin <독립 승인 SHA256>`, `--input-set <bundle>`, `--home <새 S3D1 home>`, 최초 create의 `--bootstrap <동일 H snapshot>`를 그대로 사용한다. 표준 home/자동 fallback 금지.
- `--bind <literal loopback IP:port>`, `--rpc <literal loopback IP:port>`, `--rpc-timeout-ms <1..2000>`를 요구한다. DNS/proxy/redirect와 공개 주소를 쓰지 않는다. 기존 승인 origin은 `http://127.0.0.1:5173` 또는 `http://localhost:5173`다.
- launcher가 두 opt-in을 확인해 `Rest::new(engine, validated_snapshot, Options)`에 전달한다. C `Validated/Engine` 자체도 profile·ack·runtime·guard를 검증한다. `Worker`는 이 검증으로 생성된 `Arc<Engine>`만 받는다.
- `Rest::handle`은 직접 handler이고 peer는 실제 transport socket의 IP다. browser가 보낸 peer/owner/observation을 복사하지 않는다. 원 header 중복을 보존해서 전달한다. listener는 body 16KiB, header 수32/각4096B 이하를 읽기 전에 제한하고, 중복 Content-Length/Transfer-Encoding·HTTP framing 모호성을 거절해야 한다. 모든 응답에 `Cache-Control: no-store`, JSON content type과 정확한 origin만 사용한다. 전달 헤더로 client IP를 바꾸지 않는다.
- `Observation`/Unix ms는 로컬 신뢰 Chain adapter가 제공한다. 기존 인증기는 초 단위이므로 handler 내부에서만 `now/1000`을 사용한다. RPC 단절 시 오래된 관측을 새 received_at으로 갱신하지 않는다.
- 운영자 signer는 별도 로컬 runtime의 비공개 메모리 자원이다. REST 인자·응답·일반 로그·artifact에 secret을 넣지 않는다. 제공되는 `OperatorSigner`는 키 로딩/저장/생성/설치 API가 아니다. 공개 fixture seed는 시험 전용이며 runtime에서 사용하지 않는다.

L-R의 실제 binary/웹 build·manifest와 CTO→Security 승인이 완료된 뒤 L-T가 listener 및 서비스를 시작한다. 여기의 합성 descriptor/test pin은 component 시험 입력일 뿐이다.

## worker와 방송 intent

1. `Worker::reconcile(Command::Seal("NORMAL"), …)`이 기존 C FIFO·8 fills/16 proofs·131072B·만료 여유를 적용해 배치를 commit한다. `chain::sealed_batch`는 내부 immutable revision의 원 order/signature/fill에서 그 배치만 재구성하고 **C가 이미 정한 identity와 바이트를 대조**한다. 새 batch ID/seq를 임의로 만들지 않는다.
2. `Worker::prepare_settle`은 기존 미해소 시도와 Snapshot 신선도를 서명 전에 확인한다. 내부 `chain::settle_attempt`는 검증 Snapshot, 같은 체인의 operator Account 조회 결과(account number/sequence), 원 Batch, 로컬 signer로 Cosmos `SIGN_MODE_DIRECT` TxRaw를 만든다. operator 공개키/address 및 ML-DSA 서명을 다시 확인한다. SETTLE gas10M/fee20000 DEVGAS/timeout H+8이다. B type URL·protobuf tag 및 기본값 생략을 지킨다. CLOSE는 아래 `prepare_close`가 C에 commit된 원 실패 증거에서 서명·영속 준비한다. 자동 CLOSE/VOID는 없다.
3. `Worker::prepare` → C `Command::Attempt`가 원 Batch/TxRaw/attempt를 먼저 저장한다. 미해소 시도가 있으면 새 봉투는 C가 거절한다. 원문은 C의 검증된 object store에 있고 별도 약한 D journal은 없다.
4. `broadcast`는 단일 lane에서 기존 attempt를 읽고 `SUBMISSION_UNKNOWN`, `broadcast_count+1`을 **C Resolve commit으로 먼저 저장**한다. 이 전이가 D의 방송 intent다. 전송 전에 죽어도 시도 횟수는 소비하며, 상속 지연0/1000/2000ms를 writer lock 밖에서 적용하고 최대3회 원 TxRaw 전송 이후 자동 재시도하지 않는다. 재시작에도 영속 counter에 해당하는 전체 지연을 다시 기다린다. `broadcast_count`는 socket 성공 횟수/체인 성공 횟수가 아니라 보수적인 전송 시작 의도 횟수다. 실제 effect 시작 이전에도 불명으로 보류하는 것은 기존 PREPARED crash 의미와 같다.
5. commit 응답이 완료된 뒤에만 `with_committed_attempt`에 들어간다. writer lock 안에서 stored attempt가 방금 commit한 intent와 같은지 확인하고 C가 검증한 원 TxRaw만 `LoopbackRpc`에 준다. `committed_attempt` 조회 반환을 방송 token으로 쓰지 않는다.
6. production callback에는 구체 `LoopbackRpc`만 있다. Engine 재진입·signer·임의 사용자 callback이 없다. connect/write/read 전체 deadline≤2초, TxRaw≤139264B, response≤65536B, DNS/redirect/proxy/무한 재시도 없음. 시험 callback은 `fault-injection` feature에만 있다. socket/CheckTx/응답 유실/NOT_FOUND는 전부 기존 UNKNOWN 보류로 남으며 C/R/D/P를 해제하지 않는다.
7. `chain::decode_snapshot`은 B S3 ABCI의 **canonical JSON value**를 읽는다(S2 protobuf wrapper와 다름). Context·ABCI/JSON 높이·snapshot hash·보존식은 B/C parser로 검증한다. 확정 TX·원 receipt·동일 H C와 raw block/results/TxRaw는 trusted adapter가 `Worker::reconcile`에 넘기며 C의 기존 proof 검증을 그대로 통과해야 한다. browser body를 trusted command로 변환하는 route는 없다.

출금 경합·다른 미해소 시도·원 receipt·동일 H가 해소되지 않은 상태에서 RejectFinal/Apply를 요청해도 C가 검증한다. D에는 proof 생략, timeout 정정, COMMITTED 역전, 파일 수리·자동 truncate API가 없다. RECOVERY_REQUIRED에서는 C writer가 prepare/resolve/withdraw/effect를 닫는다. 원 signed 결과는 읽기 전용 `query_signed`로 복구 조회한다.

## E용 REST 계약

모든 private route는 `/dev-local/v1/` 아래에 있다. GET에도 session bearer와 허용 Origin이 필요하다. 모든 정수는 문자열이다. 요청 JSON은 중복/추가 필드를 거절한다. owner는 session에서 얻으며 요청 본문 owner는 받지 않는다.

| method / suffix | 요청 | 응답 의미 |
|---|---|---|
| POST auth/challenge | 기존 `{owner,origin,audience:"exchange-api"}` | 기존 WalletChallengeV1 원문 |
| POST auth/session | `{wire_base64,signature_base64}` | 기존 ML-DSA 인증 후 탭 메모리 bearer; 300초 |
| POST auth/logout | `{}` | session 해제 |
| GET capabilities | body 없음 | 개발 envelope/profile/context, 보장, signed_result_query=true, automatic_withdraw=false, ws=false |
| GET account | body 없음 | 같은 revision의 본인 ledger C/R/D/P/A, orders/fills/batches, gate, freshness, cursor·관측높이 |
| POST orders / cancels | `{context,wire_base64,signature_base64}` | 서명 재확인·C commit 후 본인 결과 projection |
| POST receipts/orders / receipts/cancels | 동일 signed 요청 | `query_signed`; recovery에서도 원 결과 반환, 없으면 NOT_FOUND |
| POST withdraw/prepare / withdraw/abort | `{context,request_id}` | 기존 C LocalAction만 수행. 사용자 출금 TX 생성/방송 아님 |

`account`는 `reader().get()`을 직렬화하지 않는다. 본인 order view, 본인 fill, 그 fill이 속한 batch만 선택한다. 다른 owner/서명/order wire/keys/result map/evidence refs를 제외한다. BatchIdentity의 다른 fill ID 목록도 생략한다. 체인 영수증은 `disposition/terminal_height/terminal_tx_hash/batch_receipt_v2`를 노출한다. `fills[].state`는 PENDING/SUBMISSION_UNKNOWN/COMMITTED/CORRECTED를 유지하며 수취액 P를 C 또는 A에 합치지 않는다.

변경/조회 응답은 C 개발 envelope의 보장 메타데이터와 **본인 command_result projection**이다. 내부 C CommandResult/원 receipt ledger는 수정하지 않는다. public projection에는 command_seq/kind/request_hash/code/state/observed_height/snapshot_id 및 본인 ledger_changes만 있다. 다른 계정 ledger나 내부 state hash를 원 public receipt인 것처럼 재노출하지 않는다. E는 개발 접수를 chain COMMITTED로 해석하면 안 된다.

`fresh`는 C snapshot의 원 2초 query/5초 관측·block age 기준을 재사용한다. `withdraw_ready`는 fresh·OPEN·본인 freeze 완료·R=D=P=0일 때만 true다. revision gap/역행·계정 전환의 캐시 처리는 E가 수행한다. `fresh=true`만으로 출금 가능 또는 정산 확정이라고 표시하지 않는다.

오류는 auth401, origin/peer403, path/result404, method405, 크기413, 입력/경합409, recovery503이다. IO/OS 경로와 raw error는 공개 응답에 넣지 않는다. recovery 중 신규 주문 재시도는503이며 원 결과 조회 route를 따로 사용한다.

## 검증과 범위

현재 component 시험은 실제 C store/WAL/marker, 공개 합성 계정의 실제 ML-DSA, 직접 REST handler, 합성 B RPC bytes, subprocess 종료를 사용한다. HTTP/RPC 서비스 기동·4검증인·브라우저·실제 host ENOSPC·운영 내구성은 이 시험의 PASS 대상이 아니다. 원 계약과 lock은 그대로다.

재현(설치된 Rust1.92.0과 기존 cache만 사용):

```sh
cargo test --offline --locked --manifest-path exchange/Cargo.toml \
  --features dev-local-settlement,fault-injection \
  --test s3_candidates settlement_ -- --test-threads=1
cargo test --offline --locked --manifest-path exchange/Cargo.toml \
  --features dev-local-settlement,fault-injection \
  --test s3_dev_local --test s2_request -- --test-threads=1
cargo build --offline --locked --manifest-path exchange/Cargo.toml \
  --features dev-local-settlement --lib
cargo check --offline --locked --manifest-path exchange/Cargo.toml \
  --no-default-features --lib
```

`PAPERCLIP_RUN_SCRATCH_DIR` 또는 `NUS_TEST_TMPDIR`에 쓰기 가능한 임시 경로가 필요하다. 새 component 원시 증거는 `S3_CANDIDATE_EVIDENCE_DIR`로 수집한다. 남은 실제 DEV05/DEV12/DEV01~14 통합 판정은 L-T, 기동 전 runtime pin은 L-R에 남는다. 전문 검토가 끝나기 전 본 업무도 done으로 인수하지 않는다.

## CLOSE 후속 API — C `8bbacf9` 실패 복구 연결

`Worker::prepare_close(expected_commit, snapshot, batch_id, attempt_no,
account_number, account_sequence, signer, observation, now)`는 신뢰하는 로컬
runtime 전용이다. REST route나 요청 body를 추가하지 않는다. 성공 반환은
TxRaw hash이며 방송이나 VOID/정정/잔고 해제를 의미하지 않는다.

호출 순서:

1. 같은 C view의 commit과 최신 Snapshot을 고정한다. 기존 B Account adapter로
   그 Snapshot 높이의 현재 operator Account를 조회하고 `Account::at(snapshot,
   operator)` 검증 후 number/sequence를 전달한다. **이 API의 정수 인자는 신뢰
   adapter 입력이며 Account RPC 원문의 인증·높이 검증을 대신하지 않는다.**
   account 조회에 걸린 시간을 포함한 원 Observation으로 now를 계산한다.
2. `prepare_close`는 expected commit, latest snapshot/Context, 원 freshness,
   REJECTED_FINAL/CLOSING, batch의 모든 attempt를 확인한다. PREPARED/UNKNOWN은
   서명 전에 거절하고 CLOSE는 최대2개, 다음 attempt_no만 허용한다.
3. `trusted_recovery_attempt`와 `trusted_recovery_failure`는 동일 expected commit으로
   호출한다. C가 저장된 원 ResolutionEvidence 원문·참조·closure를 재검증한다.
   누락·손상은 C recovery로 닫힌다. D는 실패를 재선택하거나 closure를 재계산하지
   않고 저장된 원 실패의 failed_tx_hash와 NUS/S3/RESOLUTION_EVIDENCE/V1 hash를 쓴다.
4. 이미 sealed된 Batch의 identity를 재검증하여 B `MsgCloseBatch` tag1 operator,
   tag2 exact BatchV2, tag3 failed TX hash32B, tag4 resolution hash32B를 만든다.
   SIGN_MODE_DIRECT, ML-DSA65, gas3000000, fee6000 DEVGAS, timeout H+8,
   first_possible H+1 및 기존 TxRaw cap을 유지한다. 공개키/operator·반환 서명을
   검증하며, signer는 writer effect lock 밖에 있다. signer 중 다른 commit이
   생겼으면 바이트를 버리고 STALE_COMMIT으로 거절한다.
5. C `Command::Attempt`가 원 TxRaw와 Attempt를 commit하고 authoritative 상태·원문·
   실패 audit binding을 writer lock 아래 다시 검증한 뒤 hash를 돌려준다. 다른
   worker와의 최종 경합도 C가 판정한다. 다음 방송은 기존 `broadcast`의 영속 intent,
   원문 재확인, writer lock 안 bounded RPC를 사용한다. 서명 성공 자체는 방송 허가가 아니다.

응답 유실 후에는 commit된 CLOSE를 C trusted recovery API에서 찾아 기존 hash로
재개한다. PREPARED도 미해소이므로 새 봉투 생성은 거절한다. 확정 종결 후에만
두 번째 CLOSE가 허용되고 원 실패 증거는 최신 snapshot으로 다시 만들지 않는다.
저장 도중 불완전 객체가 남으면 자동 수리 없이 recovery로 닫힌다. commit 후 응답
유실이면 같은 home 재생으로 정확한 Attempt가 복원된다. CLOSE 체인 영수증/VOID
Apply의 실제 배선과 기동은 L-R/L-T에 남는다.

후속 시험 `settlement_close`는 fee0/25, 재시작 뒤 실제 합성 ML-DSA 서명, exact TX,
원 실패 hash 결합, stale/다른 batch/원문 손상/미해소/2회 예산/다른 signer 거절,
현재 Account number·nonzero sequence의 두 번째 CLOSE, signer 중 commit 경합,
저장 실패 callback0 및 두 번 재생을 다룬다. 원시 JSON/store는
`NUS70_EVIDENCE_DIR`, 기존 후보 trace는 `S3_CANDIDATE_EVIDENCE_DIR`로 수집한다.
실제 HTTP/4검증인/DEV 통합은 여전히 NOT_RUN이며 새 runtime pin을 발급하지 않는다.
