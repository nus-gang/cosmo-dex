# S3 raw 증거 저장·최악 정정 예약 (rc3)

[NUS-65](/NUS/issues/NUS-65) · CTO · Security→QA 재심사 후보. s3/3의 규범이다. 이는 공통 저장 계약·검증 oracle이며 Exchange의 엔진/fsync 구현이나 실제 Chain proof가 아니다.

## 1. 버전과 원 bytes

s3/3 Context/profile/marker를 새 빈 S3 home에 함께 활성화한다. s3/1·s3/2 journal의 암묵 변환, 기존 hash 재봉인, S1/S2 데이터 import는 금지한다. 사용자/BatchV2 wire·서명·S3W1 72B header·16,777,216B payload ceiling과 language lock은 변경하지 않는다.

작은 응답도 예외 없이 raw object를 쓴다. `ConfirmedTx.raw_tx_ref`, `Attempt.raw_tx_ref`는 TxEvidenceRef이고 `ConfirmedTx`/`AbsenceBlock`의 `raw_block_response_ref`, `raw_results_response_ref`는 RpcEvidenceRef다. 구 `raw_tx`, `raw_block_response`, `raw_results_response` 필드는 unknown key로 거절한다. Reference는 세 key `sha256`, `byte_length`, `media_type` 전부 required이며 URI/path/host는 받지 않는다. sha256은 lowercase hex64, 길이는 양의 U64 decimal이다. RPC는 application/json, TxRaw는 application/vnd.nus.txraw, canonical typed S3 evidence는 application/vnd.nus.s3+json으로 구분한다.

RPC HTTP 요청은 `Accept-Encoding: identity`; 다른 Content-Encoding은 채택하지 않는다. raw는 HTTP transfer framing을 제거한 수신 body의 정확한 octets다. whitespace, key 순서, escape, 숫자 표기를 trim/canonicalize/re-encode하지 않는다. 저장 digest는 `SHA256(exact bytes)`로 domain frame을 붙이지 않는다. 같은 JSON 의미라도 bytes가 다르면 다른 객체다. body를 모두 받기 전에는 proof 성공이 아니다. 중복 key·비정상 JSON·NaN/Infinity·잘못된 UTF-8를 거절하며 문자열 정수/chain 의미는 별도 검증한다.

| 입력/객체 | hard ceiling | 의미 |
|---|---:|---|
| raw RPC body 하나 | 16,777,216B | JSON-RPC transport 입력 정책. consensus block_max_bytes에서 추정한 값이 아님 |
| TxRaw 하나 | 139,264B | 원 서명 bytes; 기존 settle/control ceiling. 사용자 TX의 더 작은 한도도 유지 |
| canonical typed evidence 하나 | 262,144B | Attempt, ResolutionEvidence, ChainSnapshot, BatchLookup 등; 형식별 schema/의미도 만족 |
| ChainSnapshot owner_events / terminal_batch_seqs | 각각128 | 저장·조회 envelope 경계이며 블록 처리량 보장 아님 |

크기 초과·불완전 응답은 유효 proof로 저장하거나 일부만 참조하지 않는다. EVIDENCE_SIZE/RECOVERY_REQUIRED로 신규 접수·정산을 닫고 D/P·기존 ACK/시도/전용 예약을 유지한다. cap을 넘는 무한 외부 응답이나 소실된 증거의 회수 가능성은 보장하지 않는다. 완료된 지원 범위의 원 bytes는 모두 보존하며, 거절된 stream의 관측 길이/오류와 확보한 증거는 별도 비권위 기록으로 남긴다. 이 정책을 바꾸려면 새 계약/profile 재심사가 필요하다.

## 2. 저장 DAG·hash와 원자성

namespace는 기존 `(chain_id, genesis_hash, market_id)`다. 저장 상대 위치는 그 namespace의 `objects/sha256/<hex64>`로만 결정한다. 임의 경로·symlink·외부 fetch로 해소하지 않는다. 루트/디렉터리와 파일 소유권을 제한하고 단일 writer를 유지한다. 같은 digest의 기존 파일은 길이·내용 digest를 다시 확인하며 다른 metadata/bytes이면 닫는다. rename은 동일 파일시스템의 기존 객체를 덮어쓰지 않는 방식이어야 한다.

순서: 수신/생성 exact bytes → 전용 예약에서 임시 object 공간 배정 → 전체 write → 파일 fsync → digest/길이/역할/typed 내용 검증 → no-replace publish → object 디렉터리 fsync → 이를 참조하는 canonical metadata 객체도 같은 순서로 저장 → candidate state/result 및 완전한 ref 집합 → WAL append+fsync → marker temp fsync/rename/dir fsync → 외부 공개/ACK. object가 먼저 남은 crash는 orphan 증거일 뿐 ledger 효과가 없다. WAL 또는 marker가 먼저 성공한 것처럼 공개하면 안 된다. S3에서 객체 GC0, committed object와 원 WAL은 append-only다. 임시 파일 삭제/재사용은 미커밋 여부와 예약 소유권을 확인한 복구 경로에 한정한다.

VOID `ResolutionReceipt.resolution_evidence_ref`는 non-null canonical **ResolutionEvidence 전체**를 가리킨다. COMMITTED는 null이다. canonical 객체 내부 raw는 다시 typed refs다. 다음 두 해시를 혼동하지 않는다.

- ref.sha256 = SHA256(canonical ResolutionEvidence bytes): 저장 객체의 주소.
- receipt.resolution_evidence_hash = SHA256(frame(NUS/S3/RESOLUTION_EVIDENCE/V1, 같은 canonical bytes)): MsgCloseBatch/체인 compact receipt의 기존 감사 결합값.

context/batch/failed_tx_hash/observed_snapshot/batch_lookup/모든 settle attempts가 원 journal과 일치해야 한다. proof의 BatchLookup은 close 전에 관측한 값이며 자기 VOID receipt를 포함하지 않는다. 원 실패 증거가 확정됐다고 TxRaw/원 block/원 results의 내용 검증을 건너뛰지 않는다. 실제 tx[index]/hash/code/gas/H/chain/history/receipt 검증은 CONTRACT §5 그대로다. ref가 맞는 것만으로 합의 proof PASS가 되지 않는다. chain의 compact StoredResolutionReceipt에는 raw나 storage ref를 넣지 않는다.

`EngineState.resolution_receipts[]`, 각 CorrectionRecord 및 CommandResult의 Correction은 같은 ref 기반 ResolutionReceipt를 보존한다. Engine AppliedBatch.receipt_hash는 이 서비스 ResolutionReceipt의 SHA256(canonical bytes)이며 체인 compact receipt의 hash와 구분한다. state hash는 여전히 전체 canonical state를 포함한다. bytes는 참조 hash/length/type을 통해 결합된다. 순서는 raw → Attempt/ResolutionEvidence → receipt/CorrectionRecord → state hash → Correction audit/CommandResult hash → WAL이다. evidence 객체에서 현재/미래 state/result/correction/WAL로 역참조하거나 자기·순환 참조하는 경로는 금지한다. arbitrary JSON 객체 대신 슬롯별 정확한 schema 타입을 해소해야 한다.

`JournalRecord.evidence_refs`는 state/result/snapshot 및 typed metadata에서 전이적으로 도달하는 모든 raw/metadata refs의 SHA순·중복 없는 정확한 집합이다. 같은 SHA의 type/길이 불일치, extra/누락 ref는 거절한다. 독립형 bytes `state_json`/`result_json`을 이 ref 배열로 대체하지 않는다. snapshot/ref 모두 동일 commit에 결합한다.

복구는 marker/header/payload length·record hash → context → canonical state/result decode/schema → 모든 전이 객체의 exact length/SHA/role/schema → 원 chain 증거/시도·receipt·snapshot 대조 → before state에서 결정적 경제 효과 재계산 → state와 result의 full bytes/hash 및 record/audit 1:1 대조 → 단일 공개 순서다. 해시 캐시만 보고 객체 검증을 생략하지 않는다. 과거 receipt/정정의 같은 ID retry도 증거를 확인한 뒤 원 result를 반환하고 새 revision/자산 효과0이다. 누락/변조/부분 객체는 EVIDENCE_MISSING/MISMATCH/RECOVERY_REQUIRED이며 성공 복구·D/P 해제0이다.

## 3. ACK 전 계산 가능한 용량 계약

무제한 누적 이력을 16MiB에 영원히 넣을 수는 없다. 내부 배열을 페이지200/1000으로 자르지 않으며, 미래까지 담을 수 없으면 **새 ACK 이전** STORAGE_CAPACITY로 거절한다. 기존 ACK의 정정에는 일반 여유 공간 대신 이미 확보한 전용 예약을 쓴다. 신규 명령, 자동 seal, 재시도와 같은 예약을 동시에 소비하지 못하도록 reservation ledger를 단일 writer의 WAL/marker 및 복구에 결합한다. ledger 자체는 state/result의 자기참조가 아니라 프레임 주소/별도 allocator 영역에 결합하며 crash 후 원 WAL에서 예약의 소유자·잔여량을 재구성한다.

ACK 후보 S는 해당 명령의 매칭까지 포함한 완전한 private state다. N은 S의 PENDING/SUBMISSION_UNKNOWN fill 수, O는 누적 저장 주문 수다. 새 ACK 없는 drain에서는 fill/order/binding/dependency를 새로 만들지 않는다. 정정은 하나 이상의 아직 pending fill을 terminal로 만들어야 하므로 앞으로 최대 N개다. open 잔량만의 취소는 CorrectionRecord를 만들지 않는다. 각 정정의 roots/corrected/surviving 목록은 N, affected/cancelled 목록은 O 이하로 보수적으로 잡는다. 서로 겹치지 않을 거라는 기대를 예약에 사용하지 않는다. 기존 역사적 CorrectionRecord/receipt는 exact bytes 길이를 보존한다.

`tools/capacity.py`가 아래 규칙의 실행 가능한 상한 oracle이다. float/Number 또는 평균 압축률을 사용하지 않는다.

1. 모든 변할 수 있는 U32/U64/U128의 최대 십진 자릿수, nullable의 큰 형식, 모든 enum의 최대 encoded 길이, Text의 ASCII escape **code point당 최악12B**와 양쪽 따옴표2B, 가능한 모든 properties/배열 separators를 포함한다. `maxLength`는 Unicode code point 수다. 보충 평면 U+10000..U+10FFFF 하나는 `\uXXXX\uXXXX` surrogate pair로12B이므로 Text256의 상한은3074B다. BMP escape는 최대6B이며 quote/backslash/짧은 제어 escape도12B 상한 안에 든다. 임의 pattern이나 이름으로 ASCII를 추정하지 않는다. 정확히 검토한 decimal/base64 pattern만 문자당1B, hex64 pattern은 전체66B로 계산하고 나머지 유한 문자열은12B를 쓴다. 상한 없는 문자열은 UNBOUNDED_STRING으로 닫는다. 고정 owner/key/signature/request/receipt byte 길이는 schema의 좁은 정의를 사용한다. 이미 서명한 주문 owner/wire/signature만 알려진 실제 길이를 쓴다.
2. state: 기존 orders/fills/batches를 전체 포함하고 batches/receipts/applied_batches/corrections 각각 N개, attempt refs는25N개를 추가한 최대 형식 Smax를 계산한다. bindings/dependencies는 이 drain에서 불변이다. 현재 snapshot/계정/잔고/모드/revision은 전체 최악 형식으로 계산한다. 과거 correction은 원 bytes 그대로 유지한다.
3. result: 최대 N개 완전한 audit Correction, 각각 최대 N/O 목록, 최대32 ledger changes 및 나머지 목록을 포함해 Rmax를 계산한다. 모든 audit after hash는64hex 고정 폭이다.
4. 한 pending fill당 최대1개의 별도 미래 batch로 계산한다. batch당 settle3+close2, attempt당 최대8블록×2 RPC 원문 = raw80개, TxRaw5개, canonical metadata96개를 예약한다. attempt의 PREPARED+방송 count1/2/3+terminal 각1개로 최대25개 immutable 버전을 포함한다. metadata96은 이25개와 최대40개 snapshot/관측, batch/lookup/proof 및 잔여 bookkeeping을 포함하는 **출력 상한**이다. refs 수는 retained 전이 객체 수 +181N+O+2로 잡는다. 모든 미래 raw가 cap에 도달한다고 계산하며 dedup을 예상해 공간을 줄이지 않는다.
5. `base64_size(x)=4*ceil(x/3)`. Jmax는 JournalRecord의 나머지 최악 필드 + base64 Smax/Rmax + 위 refs 집합 + 최대 N+O external_event_ids의 canonical 길이다. 72B header는 payload와 별도다. Jmax≤16,777,216이어야 새 ACK 가능하다. schema에 길이/개수 상한 없는 미래 필드를 발견하면 UNBOUNDED_ARRAY/TYPE으로 작성자 검증을 실패시키며 기본0이나 page cap을 넣지 않는다.

일반 폴링/같은 결과 retry는 새 내구 레코드·객체를 계속 만들 수 없다. snapshot을 읽을 때마다 commit하는 동작은 이 예약에서 허용하지 않는다. 이미 ACK한 집합의 drain은 batch당 seal, freeze, 최대5 prepared+15 broadcast count+5 terminal, reject/closing/VOID/apply/cursor/checkpoint를 합쳐 최대40 기록, open 잔량 종료 O개와 고정2개를 합친 **Q=40N+O+2** 기록 안에서 수행한다. 이를 넘는 추가 운영 기록은 별도 일반 공간으로 처리하고 전용 예약을 소비하지 않는다. 새로운 ACK/주문이 있으면 더 큰 후보의 예약을 먼저 확보한다. 원시 proof를 바꾸는 무한 반복 조회를 예약으로 인정하지 않는다. cap 안의 정규 drain을 구현할 수 없으면 C/D의 인수 실패이며 상한을 임의 증액하지 않는다.

### 실제 공간 예약

A(x)=ceil(x/4096)*4096+8192로 파일별 allocation·metadata overhead를 예약한다. underlying allocator는 이 수치로 실제 blocks/metadata 및 directory 갱신 공간이 확보됨을 입증해야 한다. 단순 statvfs/free-space 읽기, sparse truncate, 메모리 카운터만으로는 예약 성공이 아니다. 경쟁 writer/다른 서비스가 이 공간을 쓰지 못하는 preallocation/격리 공간이 필요하다. 플랫폼 overhead가8KiB를 넘거나 보장이 없는 파일시스템에서는 지원하지 않는 것으로 닫고 SRE/CTO 검토를 요구한다.

- frame/checkpoint/marker 한 기록의 상한: `A(72+Jmax)+2*A(Smax)+2*A(Rmax)+2*A(4096)`.
- 미래 증거: `N*(80*A(16777216)+5*A(139264)+96*A(262144))+(O+2)*A(262144)`.
- 전용 예약 B = Q×기록 상한 + 미래 증거. 이미 커밋한 파일은 reusable free space로 세지 않는다. 모든 숫자는 checked integer, overflow는 접수 거절이다.

새 ACK는 (a) 실제 현재 frame도 한도 내, (b) Jmax 한도 내, (c) B의 아직 확보하지 않은 차액을 실제로 확보, (d) 예약 소유권/소모/marker가 durable한 후만 보낸다. 불필요한 이전 예약을 먼저 풀고 새 예약을 잡는 window는 금지한다. 기존 객체나 적은 시도 덕분에 남은 크레딧은 확정 terminal 적용/marker 뒤 실제 drain 상태에서 재계산한 후만 일반 공간에 돌릴 수 있다. 이미 ACK된 정정은 일반 free=0이어도 자기 크레딧에서 진행한다. IO 결과 불명/하드웨어 오류는 RECOVERY_REQUIRED이며 ACK 내구성을 과장하지 않는다.

큰 raw에 대한 보수적 예약은 pending fill1개당 약1.3GiB 이상이다. 이는 실제 사용량·비용/납기 약속이나 유료 자원 요청이 아니다. 공간이 없다면 신규 ACK를 거절한다. 추후 더 촘촘한 상한/다른 저장 형식은 새 검토 대상이다.

## 4. 검증 범위와 인계

`check.py`/`check_state_hash.py`/`check_evidence_capacity.py`는 표준라이브러리로 실행한다. 작성자 fixture의 합성 contract hash11.. 및 가짜 block/TxRaw 식별자를 실제 SDK 증거로 해석하지 않는다.

- 보고된 3,145,728B JSON whitespace bytes/digest를 그대로 보존하고, receipt/state/audit refs와 full state/result hash를 계산한다. 이전 rc2 WAL16,874,108B가 rc3에서는 fixture의 정확한 크기로 줄어든다. 단순 한도 증액이나 raw 변형은 없다.
- 원 bytes 누락/절단/변조/canonicalization, 잘못된 hash/길이/type, duplicate JSON key, 구 inline raw, 전이 ref 누락을 거절한다. 1회/2회 정정·과거 원 receipt·state/audit 해시·두 번 재생 모델을 유지한다.
- 누적1000/1001 fills·200/201 orders·0/25bps는 전체 schema 저장 크기와2개 pending의 최악 예약을 함께 검사한다. 기존 모든 pending의 의존 폐쇄·0/25 fee oracle도 유지한다. 모든1000/1001 fills가 동시에 pending인 후보의 보수적 최악 N번 정정은 한도를 넘으므로 신규 ACK를 거절한다. 하나의 closure 성공 사례를 모든 미래의 크기 보장으로 오인하지 않는다.
- WAL payload16MiB/+1과 RPC body16MiB/+1, 예약 정확한 B/B−1, 일반 여유0일 때 전용 크레딧 사용을 검사한다. framing 경계 fixture는 canonical JournalRecord **형상** 검사이며 decoded engine state나 실제 IO 수락을 주장하지 않는다.
- SEC-65-01 회귀는 BMP·보충 평면·제어문자·quote/backslash·혼합 Text의0/1/255/256 code points,257 거절, 전체 state/result→base64 WAL의 실제 bytes≤Smax/Rmax/Jmax, allocation 반올림 후 B 및 B−1/기존 전용 예약을 검사한다. Security의 원 schema-valid128901B state를 그대로 재구성하며 경제 실행 trace나 실제 공간 부족 재현으로 주장하지 않는다. 중첩 receipt/정정/미래 evidence의 최악 Text도 포함한다.

실제 fsync/preallocation/ENOSPC/crash/replay, 타 프로세스 경쟁, 실제 RPC/SDK 의미 검증과 C/D/F/G/J 제품 검증은 NOT_RUN이다. Exchange가 exact 승인 head/tree/contract/config/vector/lock을 소비하고 자기 allocator·엔진 구현으로 이를 입증해야 전체 크기 durable ACK와 D 인계를 완료할 수 있다. Security→QA 두 승인 전 NUS-56 blocker를 해제하지 않는다.
