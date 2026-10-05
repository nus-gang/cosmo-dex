# S3 비공개 엔진 후보 상태 — NUS-56 중간 구현

2026-10-05. 이 코드는 **서비스 또는 durable ACK 경계가 아니다.** `s3::sequencer::Candidate`는 원자 공개 전에 검증할 비공개 상태를 계산한다. 승인된 raw 증거 저장·정정 용량 계약을 받기 전에는 S3 runtime/REST/정산 worker에 접속하지 않는다. S2 실행 파일과 기존 journal/outbox에는 연결하지 않았다.

[NUS-64](/NUS/issues/NUS-64)의 `d45be33029705859b07a9516fd2229a56dc66f46`을 전용 branch에 병합했다. `s3/2`에서 CorrectionRecord는 after hash를 포함하지 않고, 완성된 상태 전체를 해시한다. 이전 `s3/1` journal은 자동 변환 없이 거절한다. 공통 protocol, 기존 lock, main, 다른 담당 branch를 수정하지 않았다.

## 이번 구현

- `schema`: 승인 JSON schema의 필수/미지 필드, 정수 U32/U64/U128, hex·base64·enum·배열/문자열 한도, unique JSON 입력 검증.
- `snapshot`: manifest가 제공한 context·등록 사용자 집합·공급·fee profile에 결합한다. owner/key, 두 자산의 C/T/U/module/supply 보존, 연속 높이·epoch event를 검증한다. 관측과 적용 C는 분리한다.
- `sequencer`: S2 ML-DSA 주문/취소·ID binding·FIFO/IOC/STP 어댑터를 재사용하는 S3 후보 경로다. 새 만료 여유20..1000, P 사용0, 전역 예약과 원서명을 유지하며 fill마다 네 domain의 선행 의존을 기록한다.
- `engine`: 오래된 FIFO prefix의 immutable BatchV2 seal, 한 미확정 slot, PREPARED 이후 불명 보류, 제한된 attempt 전이, 원 settle 실패와 전체 시도 대조, VOID+폐쇄, 잔량 종료와 최신 C 기준 R/D/P 재합산을 계산한다. COMMITTED/정정 수량은 lifetime과 별도로 보존한다. 독립 fill의 ID·원서명·가격을 다음 seq/원 VOID hash로 재구성한다.
- `wire`: 고정 V1 field layout을 재사용하되 batch/receipt version2와 V2 hash domain을 검증한다. SDK TxRaw의 메시지·operator·batch bytes·timeout·sequence·gas/fee를 attempt에 대조한다. 운영자 서명 권한은 체인, account number/sequence 조회와 가스 예산 확보는 D의 책임이다.
- `proof`: 신뢰 로컬 RPC raw block의 chain/height/hash/TX index/원 TX와 raw results code/gas를 대조한다. 연속8블록 불포함과 timeout 이후 관측만 attempt 만료 해소로 처리한다. 그것만으로 실패 배치 정정은 허용하지 않는다. light client proof가 아니다.

`observe`는 이전 C/R/D/P와 별도 관측 목록을 가진 frozen candidate를 만든다. `record_receipt`는 원 terminal 증거를 고정하고 경제 효과를 내지 않는다. `apply`가 현재까지 필요한 receipt와 같은 최신 H의 C·잔량/정정 전체를 함께 계산한다. 원자 공개는 **아직 없는 상위 journal service**가 해야 한다. 중간 `Candidate`를 API 잔고·확정 수취 또는 ACK로 노출하면 안 된다. 실제 RPC I/O, 로그인/HTTP 세션 권한, 운영자 계정 조회, 독립 ACK ledger는 이 모듈의 역할이 아니다.

## 계약 차단점

[NUS-65](/NUS/issues/NUS-65)가 실제 선행 blocker다. `ResolutionReceipt`의 raw block/results가 state.resolution_receipts, state.corrections, result.correction_results에 반복되고 state/result가 WAL에서 다시 base64된다. 승인 schema를 통과하는 3,145,728B raw JSON 응답으로 WAL payload가 16,874,108B가 되어 16MiB보다 96,892B 크다. 원문 JSON에 의미를 바꾸지 않는 whitespace를 추가한 크기 재현이므로 consensus block 크기 제한으로 제거할 수 없다.

CONTRACT §8은 큰 원문을 fsync 객체와 ref로 보관하도록 하나 rc2 저장 receipt 필드에는 ref 대체 타입이 없다. 입력을 임의 절단/정규화하거나 field를 지우거나 최대 payload를 늘리지 않았다. CTO가 저장 규범·hash·최악 예약과 fixture를 정정하고 Security→QA 승인을 완료해야 durable service를 연결한다.

## 검증과 제한

`evidence/s3-integration/REPORT.md`와 원시 로그가 판정 원본이다. 새 후보 시험6개는 실제 공개 시험 키의 ML-DSA 서명과 합성 JSON-RPC 증거를 사용한다. 0/25bps COMMITTED·차액2,000,000 QUOTE atoms 해제·P 사용 거절·불명 유지·변조 거절, 방향성 정정3개/독립 survivor1개와 다음 slot, 8블록 불포함·재봉투/정정 불가를 검증한다. rc2의 누적 상태 해시는 승인 fixture에 대조했다.

내구 ACK·전체 EngineState/CommandResult/Correction audit WAL·의미 재실행·stream cursor 원자 공개·통합 crash/replay는 **미완료/NOT_RUN**이다. 기초 journal의 파일시스템 crash 시험 PASS를 전체 엔진 PASS로 환산하지 않는다. signed HTTP 동시성·실제 체인·브라우저·D/F 통합·독립 심사는 NOT_RUN이다. 원장/그래프의 누적1000/1001 fills·200/201 orders 시험은 합성 이력 회귀이며 새 서비스의 용량 보장이 아니다.

## 재현

승인 lock/cache와 Rust1.92.0을 사용한다. run scratch 또는 별도 자신의 임시 디렉터리만 시험에 사용한다.

```sh
cargo +1.92.0 test --manifest-path exchange/Cargo.toml --locked --offline --test s3_candidates --test s3_accounting --test s3_journal --test s2_ledger --test s2_journal --test s2_sequencer --test s2_snapshot -- --nocapture --test-threads=1
cargo +1.92.0 clippy --manifest-path exchange/Cargo.toml --locked --offline --all-targets --all-features -- -D warnings
python3 protocol/s3/tools/check.py
python3 exchange/evidence/s3-integration/capacity_probe.py
```

`S3_CANDIDATE_EVIDENCE_DIR`를 지정하면 시험용 signed 주문·batch/TX·원 JSON-RPC·전후 후보 상태를 JSON으로 남긴다. 같은 fixture 반복 결과가 다르면 실패한다. 실제 운영자 비밀키·S1/S2 키는 읽거나 저장하지 않는다.
