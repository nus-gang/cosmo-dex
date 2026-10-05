# NUS-56 S3 후보 상태 연결·계약 용량 재현

2026-10-05 · Exchange · **중간 구현, NUS-65 계약 정정 대기**

NUS-64의 Security→QA 승인 head `d45be33029705859b07a9516fd2229a56dc66f46`, tree `8e54fdd823e984e32dd6c44ad10ef4475b81aa7f`를 기존 NUS-56 `6aec6b1` 전용 branch에 병합했다. 승인 main 기준선은 `bd9e473196ac86fdedf655b2c93e6931f54faa83`이다. 원격 main 병합과 공유 checkout 변경은 없다. 적용 code/tree는 전달 `candidate.json`과 commit work product에 별도로 기록한다.

## 구현 결과

S3 `s3/2` schema/manifest-bound snapshot, 실제 V1 서명 주문·취소/FIFO와 S3 의존 그래프, BatchV2 seal·TxRaw 결합·한 미확정 slot, raw block/results 증거 검증, 보류 관측·COMMITTED 대사 후보·VOID 방향성 폐쇄/독립 fill 재구성을 추가했다. 기존 journal context를 rc2로 바꿨으며 rc1을 자동 변환하지 않는다. 상세 책임과 아직 연결하지 않은 상위 경계는 `exchange/S3-CANDIDATES.md`를 따른다.

## 판정

| 검증 | 결과 | 해석 |
|---|---|---|
| 새 `s3_candidates` | 6 PASS | 실제 공개 시험 키의 ML-DSA 서명 + 합성 raw RPC의 private candidate 시험 |
| S3 기존 회계/그래프 | 7 PASS | 0/25bps·1000/1001 fills·200/201 orders 합성 회귀 포함 |
| S3 journal | 16 PASS | helper1 포함, storage crash6×3회×2재생. 전체 엔진 의미 재생 아님 |
| S2 원장/journal/sequencer/snapshot | 68 PASS | 5/14/40/9, journal helper1 포함 |
| clippy all-targets/all-features | PASS | `-D warnings`, 의존성/lock 변경0 |
| 상속 계약 oracle | 13,283 PASS | 명세/fixture 검사. 제품 결과에 합산하지 않음 |
| raw proof 저장 용량 | **FAIL** | schema 적합 input이 WAL 상한 초과; 수정 요청 근거 |
| durable ACK·통합 semantic crash/replay | **NOT_RUN/미완료** | 용량·저장 계약 승인 후 연결 필요 |
| 실제 chain·HTTP·브라우저·D/F·독립 검토 | **NOT_RUN** | 실제 genesis hash=null, 실자산0 |

suite 결과97은 subprocess helper2를 포함한다. child helper 실행 로그를 별도 성공 건수로 더하지 않았다. 원 S3-AT01~09 전체 PASS나 T01~T16 인수 또는 성능을 주장하지 않는다.

0/25bps 정상 후보는 각각3회 반복했고 같은 raw state/hash를 비교했다. 매도2000lots@10000, 매수1000lots limit12000, maker10000을 체결하고 잔량 취소 뒤 불명 중 매수 D_QUOTE=12000000/A=88000000을 유지한다. COMMITTED+같은H C의 후보 대사에서만 D/P=0, A_QUOTE=90000000으로 차액2000000을 해제한다. P로 추가 매도 예약은 거절한다. 중복 receipt/적용의 추가 효과0, COMMITTED→VOID 거절을 확인했다.

방향성 정정은3회 반복했다. A의 같은 주문으로 F1→F2, C의 QUOTE domain으로 F2→F3를 연결하고, B의 기존 BASE/D의 QUOTE를 쓰는 F4는 독립으로 남긴다. 확정 실패와 원시 proof+VOID 이후 앞3개만 정정하고 F4 ID/서명을 유지한 채 seq2/원 VOID hash로 seal한다. 8블록 불포함 시험1회는 gap·timeout동일높이·TX발견·다른 block hash를 거절하고, 완전한 absence는 새 envelope를 허용하지만 정정 근거가 되지 않음을 확인했다.

## 새로운 first-class blocker

[NUS-65](/NUS/issues/NUS-65)의 CTO가 저장·용량 규범을 수정하고 Security→QA 네이티브 검토를 완료해야 한다. 초기 plan과 두 review 단계가 생성된 것을 확인했고 NUS-56 blockedByIssueIds에 연결했다. NUS-64는 승인 완료된 선행으로 소비했다.

`ResolutionReceipt`가 state.resolution_receipts, state.corrections, result.correction_results에 원 raw JSON을 반복하여 내장한다. state/result의 재-base64까지 계산하면 schema가 허용하는 3,145,728B JSON 응답 하나로 payload가16,874,108B, 상한16,777,216B보다96,892B 크다. `capacity_probe.py`는 JSON 의미를 바꾸지 않는 whitespace로 원문 크기를 만든다. 실제 블록/체인 proof를 만들었다는 뜻이 아닌 schema+직렬화 크기 재현이다. consensus block 크기는 JSON-RPC 응답 whitespace 한도가 아니다.

CTO는 content-addressed ref를 저장 receipt/정정에 적용하는 타입·해시·누락/변조 거절·재생 규범 또는 동등한 명시적 최대 입력/예약 계약을 결정해야 한다. Exchange는 승인 없이 field 제거/raw 정규화/최대 payload 확대를 하지 않는다. 보존할 안전 조건은 ACK 전 전체 최악 정정 예약, 원 bytes 결합, 원 WAL append-only, 효과1회다.

## 재현·원시 근거

- `validation.txt`: 실행한7개 Rust suite 전체 stdout/stderr와 testcase/원시 구조화 결과. 재현 명령은 `exchange/S3-CANDIDATES.md`에 있다.
- `raw/committed-0.json`, `raw/committed-25.json`: 원 TX·receipt와 signed order를 포함한 최종 후보 상태·hash·예상 차이.
- `raw/directed-correction.json`: 원 signed fill 전 상태, settle/실패 proof/close/VOID와 정정 후 상태·hash·expected set.
- `raw/absence-proof.json`: 전체8개 block/results·저장 attempt·관측 snapshot. 모든 RPC는 합성이며 실제 chain 관측이 아니다.
- `capacity_probe.py`, `capacity-probe.json`: 승인 fixture에서 결정적으로 재생성하는 상한 초과 입력과 byte counts.
- `summary.json`: 환경, contract/config/vector/lock 해시와 raw 파일 SHA256. test genesis와 real genesis를 구분한다.
- `contract-oracle.txt`, `clippy.txt`: 최종 검사 원시 출력.

LOCAL_FSYNC 기초 storage만 시험했으며 내구 ACK 서비스, full CommandResult/Correction audit WAL 조립, bootstrap/command 재실행·cursor/stream 원자 공개, 정정 공간 계산과 모든 semantic crash 경계는 미완료다. 제시한 candidate 전이를 공개 서비스의 ACK나 출금 가능액으로 사용하면 안 된다. 승인 계약 소비 후 Exchange가 구현·검증을 계속하고 자기 업무의 CTO→Security 검토로 보낸다.
