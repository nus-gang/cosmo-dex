# S3 데이터·상태·오류 계약

`schema.json`은 S3 추가 envelope의 모든 필드를 required로 두고 추가 key를 금지한다. null은 schema가 허용한 곳만 가능하다. context의 `service_schema=s3/2`이 없으면 S2 또는 구 JSON으로 해석하지 않고 거절한다. 실제 hostile JSON decoder는 duplicate key도 거절하고 문자열 정수의 U32/U64/U128 상한을 따로 검사한다. JSON Schema의 maxLength만으로 숫자 범위 검사를 대신하지 않는다.

## 저장·hash 형식

S2와 같이 canonical JSON은 ASCII escape, key 사전순, 공백0, JSON boolean/null, 정수 문자열이다. 배열은 의미상 지정 순서를 보존한다. raw bytes는 canonical padded base64, hash는 lowercase hex64. hash는 length frame을 쓴다. `snapshot_id=SHA256(frame(NUS/S3/CHAIN_SNAPSHOT/V1, snapshot_id만 제거한 ChainSnapshot))`; state/result/view/evidence hash 도메인은 각각 `NUS/S3/ENGINE_STATE/V1`, `NUS/S3/COMMAND_RESULT/V1`, `NUS/S3/VIEW/V1`, `NUS/S3/RESOLUTION_EVIDENCE/V1`. domain을 생략한 JSON SHA와 교환하지 않는다. contract manifest 집합 해시는 별도 manifest 정의를 따른다.

Context는 chain/genesis/market/market config와 contract/config hash를 모두 묶는다. 모든 저장키의 최상위 namespace는 `(chain_id,genesis_hash,market_id)`다. 유일성 키와 값은 다음과 같다.

| 객체 | 키 | 보존 값/원자성 |
|---|---|---|
| Order binding | owner/epoch/order_id | order_hash, canonical bytes, raw signature, lifetime_matched/pending/settled/corrected lots, admission_seq, state; tombstone GC0 |
| Fill | fill_id | 양측/order refs/command_seq/index/q/price/fee policy/fees/worst D/net P/origin operator epoch, 주문/예약 domain별 최신 predecessor 합집합(최대4), revision/state; 원본 immutable |
| Batch candidate | market/seq + batch_id | canonical bytes/hash/prev, ordered fill IDs, source journal hash, SEALED; 덮어쓰기0 |
| Attempt | batch_id/kind/attempt_no | 원 TxRaw/hash/account number/sequence/timeout/fee/gas/first_possible H/횟수/결과/증거 refs; 서명 bytes immutable |
| StoredResolutionReceipt (chain) | market/seq | 원 BatchIdentity + COMMITTED 또는 VOID + 최초 terminal height/TX hash + receipt bytes/감사 hash; GC0, canonical 저장4096B 이내 |
| ResolutionReceipt (worker 증거) | chain receipt hash | 위 compact chain receipt에 RPC ConfirmedTx 원시 증거를 결합한 조회·검증 결과; 실행 중 chain에 block_results를 저장하지 않음 |
| LastBatch | market | 마지막 COMMITTED **또는 VOID** seq와 원 batch_hash; operator 교체에도 연속 |
| Engine application | batch_id/terminal receipt hash | applied revision/command seq/chain snapshot/cursor/state hash; 효과1회 |
| CorrectionRecord (state) | correction_id | root/closure/영향 주문/취소 잔량/survivors/before hash/증거; after hash 없음, append-only |
| Correction (WAL result) | command_seq/correction_id | 동일 CorrectionRecord + 해당 commit의 after_state_hash; CommandResult.correction_results에 저장 |
| Raw evidence | SHA256(raw bytes) | exact bytes/length/type; 별도 fsync object와 WAL reference, 부분 파일 공개0 |

accounts는 raw owner순, account.assets 및 asset totals는 denom순. 중복/누락 owner/denom을 거절한다. registered users는 실행 manifest와 정확히 일치하는 2..16개이며 임의 runtime 추가0. owner_events는 tx_index순이고, terminal_batch_seqs는 seq순이다. proof의 raw block/results는 JSON decode를 위한 문자열이 아니라 **수신 원문 bytes**를 base64로 보존한다. 해당 bytes의 SHA/길이는 evidence manifest에 있다. 계정별 내역은 인증된 자기 계정에만 제공한다.

`ConfirmedTx`는 신뢰 로컬 RPC의 확정 포함 관측이다. index/code/gas만 조합한 합성 객체는 제품 proof가 아니다. `AbsenceProof.blocks`는 first_possible_height..timeout_height를 빈 블록도 빠짐없이 담고 observed_height>timeout을 요구한다. account sequence와 last hash는 같은 observed snapshot에서 대조한다. 모든 attempt proof가 동일 genesis/chain history에 있어야 한다. ResolutionEvidence는 observed_snapshot과 batch_lookup의 같은 H/context, journal의 모든 settle_attempts(번호순), 포함 실패 failed_tx_hash/rejection_code를 담는다. 누락된 로컬 attempt가 없어야 하며 이 객체 전체 canonical JSON을 RESOLUTION_EVIDENCE 도메인으로 해시한다. close_attempt는 원 실패 증거의 입력이 아니므로 자기참조가 없다.

ResolutionReceipt.COMMITTED는 batch_receipt_v2 non-null, failed_tx_hash/evidence_hash null, 최초 성공 TX code0을 요구한다. BatchReceiptV2의 모든 필드를 Context/BatchIdentity/terminal TX에 대조한다. VOID는 batch_receipt_v2 null, failed_tx_hash/evidence_hash non-null, 최초 close TX code0을 요구한다. Chain은 compact StoredResolutionReceipt만 원자 저장하고 worker가 확정 후 RPC block/results를 결합한다. 그 raw proof는 가스 산정의 chain write bytes에 들어가지 않는다. VOID가 단독으로 실패 또는 정정 증거가 되는 것은 아니다. read는 과거 operator epoch를 현재 권한으로 덮어쓰지 않는다.

BatchLookup은 조회H의 snapshot/LastBatch를 포함한다. seq>last이면 NOT_FOUND_AT_HEIGHT이며 receipt null, seq<=last이면 FOUND와 불변 receipt가 필요하다. 후자의 누락은 RECEIPT_INCONSISTENCY다. 요청한 genesis/market와 다른 receipt를 반환하지 않는다. 아직0인 last_seq의 last_hash는 zero32다. U64_MAX last_seq이면 새 슬롯을 만들 수 없어 시장을 닫는다.

`schema.json`은 S2 정의를 복사해 S3 context/상태/추가 필드를 모두 물질화했다. S3 Engine/WAL은 S2 StoredOrder/Binding/CommandReceipt/Book/FIFO/JournalRecord 의미를 상속하되 context를 S3 Context로 바꾼 별도 schema로 저장한다. 추가로 `batches`, `attempt_refs`, `dependencies`, `resolution_receipts`, `applied_batches`, `corrections`, `latest_observation_ref` 및 각 content hash를 같은 state commit에 둔다. S2 `HELD_S2/submission_enabled=false`를 import하지 않고 신규 fill의 export_state는 `QUEUED_S3|SEALED_S3|TERMINAL_S3`이다. Fill 상태는 `PENDING|SUBMISSION_UNKNOWN|COMMITTED|CORRECTED`이고 revision은 최초1부터 바뀔 때만 증가한다. R/D/P 및 pending fee는 원 fill로 재구축하며 중간 전이 저장을 공개 snapshot으로 제공하지 않는다. 내부 arrays에 API page maxItems를 적용하지 않는다.

## 정정 해시·계산 순서 (rc2 규범)

`EngineState.corrections[]`의 타입은 `CorrectionRecord`다. 이는 `Correction`의 **정확히 after_state_hash 한 필드가 없는 별도 저장 타입**이며 unknown key 규칙으로 그 필드의 zero/null/실제 hash 삽입을 모두 거절한다. `before_state_hash`는 직전 확정 EngineState를 가리켜 순환하지 않는다. 저장 상태에서 해시 필드를 필터링하는 projection은 정의하지 않는다. `state_hash=SHA256(frame(NUS/S3/ENGINE_STATE/V1, canon(EngineState)))`는 중첩된 과거 레코드까지 모든 필드를 그대로 포함한다. 도메인 V1은 length-frame/JSON 해시 알고리즘의 버전이며 `context.service_schema=s3/2`와 새 contract/config가 저장 버전을 구분한다.

1. 직전 확정 상태 S0를 검증하고 H0를 계산한다. 같은 correction_id가 이미 적용되었다면 최초 WAL result를 검증해 그대로 조회한다. 새 seq/revision/레코드/자산 효과는 만들지 않는다. 같은 ID의 상이한 원증거는 `RECOVERY_REQUIRED`다.
2. 실패/VOID·같은 H의 snapshot·폐쇄 집합 검증 후, 전체 상태 S1을 만든다. 신규 CorrectionRecord의 before_state_hash=H0, command_seq=이번 명령 seq, revision=1이다. 레코드 자체는 immutable이며 재정정으로 덮어쓰지 않는다. correction_id 계산의 정확한 네 key는 CONTRACT §8에 있다. root ID는 hex 사전순·중복0, 다른 목록은 기존 원순서 규칙을 따른다. corrections 배열은 (숫자 command_seq, correction_id) 오름차순으로 누적하며 과거 레코드를 그대로 보존한다.
3. 완성한 S1 전체를 canonicalize하여 H1을 **한 번** 계산한다. 새로운 각 레코드의 필드를 그대로 복사하고 after_state_hash=H1을 추가하여 `Correction`을 만든다. 여러 정정이 같은 commit에 있으면 모두 같은 H0/H1을 사용한다.
4. `CommandResult.correction_results`는 이번 commit에 append한 레코드와 1:1·같은 순서여야 한다. 정정 없는 명령은 빈 배열이며 필드 생략은 금지한다. result.after_state_hash=H1. 신규 정정의 corrected/affected 목록과 result의 해당 목록은 중복 없이 기존 원순서로 일치해야 한다. 내부 CORRECTION의 request_hash는 `SHA256(frame(NUS/S3/CORRECTION_COMMAND/V1, canon({"context": Context, "command_seq": seq, "snapshot_id": S1.chain_snapshot.snapshot_id, "correction_ids": 이번 ID 목록})))`다. 서명/wire는 기존 내부 명령대로 빈 bytes이다.
5. result_hash=`SHA256(frame(NUS/S3/COMMAND_RESULT/V1, canon(CommandResult)))`를 계산한다. `JournalRecord.state_json/result_json`은 위 canonical bytes의 padded base64다. journal before/after/result hash를 각각 H0/H1/result_hash에 대조하고 기존 WAL record hash→fsync→marker→공개 순서를 수행한다. 현재 result/result_hash/Correction.after_state_hash/WAL hash를 S1에 다시 넣지 않는다. 원시 증거 ref는 결과나 journal 자체의 ref가 되어서는 안 된다.
6. 재생은 marker 경계와 원증거를 검증하고 S0에서 결정적 S1을 재계산한다. 저장된 state bytes, H1, 각 record/audit의 1:1 필드 동일성, before/after/command_seq/context, result bytes 및 result_hash를 모두 대조한다. 과거 audit.after_state_hash는 **그 당시 상태**의 hash이며 뒤의 S2 hash로 갱신하지 않는다. state의 과거 record 변조는 state hash 불일치, state 밖 audit 변조는 audit/state 대조 또는 result hash 불일치로 실패해야 한다. result index는 기존과 같이 상태 밖에 있고 원 WAL과 대조한다.

S3 `s3/1` snapshot/journal/marker를 rc2로 자동 변환하거나 저장 hash를 다시 봉인해 채택하지 않는다. 새로운 빈 S3 저장소와 별도 실행 context만 허용한다. 사용자/Batch protobuf·서명·hash domain, 기존 S1/S2·language lock에는 변경이 없다. `vectors/correction-state-hash.json`은 고정 합성 context의 1회/2회 정정 및 두 번 재생 입력/예상 raw bytes/hash다. fixture의 `contract_hash=11..`은 manifest 자기참조를 피하는 명시적 합성 식별자이며 실제 실행은 승인 manifest hash를 context에 넣는다. 실제 체인 proof·제품 WAL IO 검증을 대신하지 않는다.

## 배치·시도 전이표

| 현재 | 입력과 필수 증거 | 다음 | D/P·가격 개선 |
|---|---|---|---|
| 미배정 pending queue | fresh/한도·만료 여유 + immutable bytes durable | SEALED | 보존 |
| SEALED | attempt durable, 방송 가능성 발생 | SUBMISSION_UNKNOWN | 보존 |
| SEALED/UNKNOWN | 검증된 COMMITTED receipt + 동일H C 대사 | COMMITTED 적용 후보 | 원자 엔진 commit에서만 해제 |
| UNKNOWN | timeout/CheckTx/NOT_FOUND/조회 오류 | UNKNOWN | 보존 |
| UNKNOWN | 한 attempt 실패, 다른 attempt 미해소 | UNKNOWN | 보존 |
| UNKNOWN | 확정 실패1+모든 다른 시도 종결+receipt 미적용 | REJECTED_FINAL | 보존 |
| REJECTED_FINAL | 예상 거절·close attempt durable | CLOSING | 보존 |
| CLOSING | VOID receipt + 원 실패 proof 검증 | correction 후보 | 원자 엔진 commit 전 보존 |
| correction 후보 | 의존 closure/전체 재계산/fsync+marker | CORRECTED | 해당 fill만 해제, 원기록 유지 |
| 어떤 미확정 상태 | 증거 불일치/예산 소진/예상 밖 실패 | RECOVERY_REQUIRED | 보존·신규 접수/방송 닫기 |
| COMMITTED/CORRECTED | 동일 원증거/동일revision 재전송 | 그대로 | 추가 효과0 |
| COMMITTED | 늦은 failure/epoch change/close | COMMITTED | 확정 fill 역전0 |

PREPARED attempt는 marker 이후 네트워크 호출 전 crash라도 조회 대상이다. unknown tail은 증거 보존·명시적 복구 외에 임의 버리지 않는다. INCLUDED_SUCCESS/FAILURE, EXPIRED_ABSENT_PROVEN만 attempt 종결이다. PREPARED/SUBMISSION_UNKNOWN은 종결이 아니다. Included success와 batch receipt의 원래 txhash가 다른 경우 같은 batch의 ALREADY_COMMITTED 또는 같은 VOID close replay인지 확인한다. 늦은 duplicate의 TX 결과를 최초 receipt에 덮어쓰지 않는다.

## Chain Query와 REST

Query는 신뢰 loopback ABCI를 통하고 SDK message routing과 분리한다. GET은 상태를 변경하지 않는다. byte query input은 canonical UTF-8 JSON으로 `context`, 필요한 seq/hash, 조회 height를 담고 duplicate/unknown key를 거절한다. `height=0/latest`는 먼저 committed H를 고정한 뒤 나머지 조회를 모두H로 수행한다. 원시 code/log와 사용자 API의 string code를 함께 증거에 남긴다.

| 앱 Query/REST 경로 | 입력/출력·권한 |
|---|---|
| `/nus.exchange.s3.v1.Query/Snapshot` | S3 Context + H → ChainSnapshot. S2 snapshot 경로와 분리 |
| `/nus.exchange.s3.v1.Query/Batch` | Context/seq/H → BatchLookup |
| `/nus.exchange.s3.v1.Query/Order` | Context/order_hash/H → binding/cumulative/revoked. operator 로컬 내부 소비 |
| Comet `/block`, `/block_results`, `/tx` | 저장 raw TX 대조·결과/과거 조회. `/tx` 미발견은 block scan으로 해소 |
| GET `/s3/network`, `/s3/status` | 공개 S3 network/context/profile 및 Status; 실제 genesis/활성 fee/한도/신선도 |
| POST `/s3/auth/challenges`, `/s3/auth/sessions` | S2 방식의 실제 WalletChallengeV1·nonce 원자 소비; S3 context 결합 |
| POST `/s3/orders`, `/s3/cancels` | owner session+실제 V1 서명. S2 receipt 멱등 semantics, S3 context |
| GET `/s3/book` | 공개 집계, 한 engine stream_seq에서만 생성 |
| GET `/s3/me` 및 `/s3/me/orders`, `/s3/me/fills` | 자기 LedgerView·원 명령receipt+최신entity revision. S2 필드에 batch identity/txhash/final height/correction reason 추가 |
| GET `/s3/markets/{market}/batches/{seq}` | 공개 BatchView/receipt identity. raw 주문·다른 사용자 내역·키·proof 원문 제외 |
| GET `/s3/me/batches/{seq}/receipt` | 자기 fill을 포함하는 배치의 PublicReceipt 및 본인 FillView만. raw proof/상대 주문 제외, 불포함은403 |
| POST `/s3/me/withdraw-prepare`, `/s3/me/withdraw-abort` | 자기 owner local command; WithdrawalReadiness. 금액 이동/서명 권한0 |
| POST `/s3/txs`, GET `/s3/txs/{hash}` | 사용자 서명된 기존 Deposit/Withdraw + Revoke/Bump만; S3 context·기존 DIRECT 검증·별도journal. operator/admin 메시지는 이 REST에서 거절 |

Chain authority/worker attempt 원문과 전체 증거는 로컬 운영·검토 artifact만 접근한다. auth/session은 S2 TTL120/300초·loopback origin 두 개·키 메모리/탭 수명·개인 GET 권한·CORS 그대로다. S3 network를 확인하지 않고 `/s1/txs`에 S3 서명 TX를 우회 전송하지 않는다. 익명 공개 receipt 조회는 원 raw 주문/signature/세션을 노출하지 않는다.

같은 snapshot 응답의 C/R/D/P/A·book·fills는 하나의 committed engine seq다. `stream_seq`와 entity revision은 U64, 중복 같은hash는 무효과, 같은seq 다른hash는 conflict로 닫기, 역행은 폐기·재조회, gap은 full snapshot까지 입력 닫기다. network/genesis/owner/account_generation이 바뀌면 늦은 응답 폐기. `fresh=false`에 마지막 수치를 확정 가용액으로 표시하지 않는다. UI 문구는 “잠정”, “제출 결과 확인 중”, “체인 확정”, “정정됨”을 구분하고 batch/TX/확정 높이를 연결한다. receipt 조회가 성공했다고 모든 잔고 snapshot이 최신인 것은 아니다.

## 오류 결정과 ABCI code

자원 → canonical/integer → version/context → 현재 TX권한 → 이미 소비한 seq의 identity → 신규 seq/prev/operator epoch → 주문등록키/서명 → binding → owner epoch/revoke/expiry → market/fee → cumulative/gross → 원자 state/write 순서다. 동순위는 주문hash 또는 fill 원순서, 필드tag순으로 최초 오류를 고른다. v1 rc4의 cap·integer 세부 순서를 보존한다. 기존 receipt 재시도에서는 신규 효과에만 필요한 주문 정책 검사를 하지 않는다.

Chain message validation의 codespace는 `exchange_s3`, numeric mapping은 `errors.json.abci_codes`로 고정한다. SDK ante errors는 원 SDK codespace/code를 보존하고 text log를 성공/실패 판단에 사용하지 않는다. ALREADY_COMMITTED/ALREADY_CLOSED는 abci code0 + 원 receipt lookup 결과다. `BatchView.reason`은 이 표/상속 오류의 string code 또는 빈문자열만 허용한다. 공개 오류에는 타인 key/account/상태를 싣지 않는다.

RESOURCE_LIMIT·정규성·권한 등의 **제출 전 거절**은 REJECTED다. 동일한 string EXPIRED라도 SDK 확정 실패 증거와 미해소 시도 확인 전에는 배치 REJECTED_FINAL이 아니다. `retryable=true`는 조회 또는 같은 bytes 재시도가 가능한 의미이고 새 batch/추가 가스 승인 의미가 아니다. 모든 alias/HTTP mapping은 `errors.json`, 상속 숫자/fee mapping은 v1 DECISION-PORT가 권위다.
