# S3-A 계약 인계

[실행 계약](CONTRACT.md) · [schema/상태/API](SCHEMA.md) · [결정·회고·작업량](adr/S3-decisions.md) · [시험 행렬](acceptance.json) · [원시 fixture](vectors/) · [출처](evidence/SOURCES.md)

상태: **Security → QA 재심사 후보**. 승인 전 rc2 의존 상태/결과 조립 금지. 기준 main `bd9e473196ac86fdedf655b2c93e6931f54faa83`, tree `31a0d1c65e9b71647cac6c4c45bf7e8d2dd9d7f3`. 이 폴더만 추가하며 기존 코드·lock·S1/S2 데이터는 변경하지 않았다. rc1 승인본은 [NUS-54](/NUS/issues/NUS-54), 비순환 해시 rc2의 head/tree·manifest·재심사는 [NUS-64](/NUS/issues/NUS-64)에 고정한다. I 이전 main 병합은 하지 않는다.

사용자 서명(Order/Cancel/Wallet)·FillIdentity는 V1을 재사용한다. Batch/BatchReceipt는 VOID 슬롯을 명시하는 wire2이며 `batch.proto`와 V2 해시 도메인·새 S3 genesis 높이1 활성 경계를 따른다. SDK wrapper는 `messages.proto`, 서비스 JSON은 `schema.json`의 s3/2이다.

## 소비자 검증

승인된 정확한 commit을 새 checkout에서 확인하고 아래만 실행한다. 소비자는 `--seal` 또는 생성 도구를 실행하여 불일치를 덮어쓰지 않는다.

```sh
python3 protocol/s3/tools/check.py
cd chain
go run -mod=readonly ../protocol/s3/tools/crypto.go check ../protocol/s3
```

Python 표준라이브러리만 사용한다. Go 검사 모듈은 기존 `chain/go.mod`의 Go1.24.4/CIRCL1.6.3이다. SDK 앱의 Go1.26.5 실행 검증과 다르다. 상속 Go decoder는 v1과 같은 **layout**의 strict roundtrip을 검사하고, S3 version2/업무 상태는 명세 oracle이 검사한다. 이 도구는 BatchV2의 제품 SDK adapter를 구현하지 않는다. Go/Rust/TS 제품 소비자는 같은 원시 bytes·hash·오류를 자신의 경로에서 비교해야 한다.

검사 범위는 정수/fee·C_start gross·예상 장부·만료와 timeout 등호·과거 receipt·불명 보류·폐쇄·의존성/원자 공개 모델·정규 bytes/hash·실제 ML-DSA·manifest다. 1000/1001 fills의 저장 predecessor를 domain별 최신 목록과 대조하고, 압축 전 전체 의존 그래프와 모든 노드의 도달 집합이 같은지도 검사한다. [rc2 정정 검증](evidence/nus64-verification.json)과 [rc1 작성자 검증 원문](evidence/verification.txt)과 [실행 환경·구조화 결과](evidence/verification.json)를 보존한다. actual chain/IO/브라우저/main 인수는 NOT_RUN이다. oracle의 의도적 잘못된 예상값 검출을 제품 결함 주입 PASS로 부르지 않는다.

## fixture 구성

| 파일 | 입력/예상값 |
|---|---|
| signed.json/test-keys.json | 새 공개 합성키, V1 사용자 서명과 SDK DIRECT SignDoc; runtime 사용 금지 |
| batches.json/batch-*.bin | 0/25bps, 실제 정책상 8fills·16orders 최대 positive, U64/U32 폭 최대 structural fixture; V2 ID/hash |
| txs.json/tx-*.bin | 같은 batch를 담은 sequence0/1의 서로 다른 실제 서명 TxRaw/hash; 합성임을 유지 |
| receipt.json/receipt-demo.bin | 해당 batch/TX에 결합한 BatchReceiptV2 예상 bytes; 체인 포함 증거가 아님 |
| negative-wire.json/negative-*.bin | 9fills/17proofs/초과bytes/duplicate/empty/unknown tag/v1 미지원 원시 입력 |
| policy.json | fee/ceil/cap/폭/expiry/timeout/C_start/receipt/attempt 판정 literal 기대값 |
| ledger.json | 대표 거래0/25, 가격 개선 보류/해제, 출금두순서, T/U/부족액 |
| correction.json/correction-history.json | F1→F2→F3 폐쇄, F4 독립, F0 COMMITTED 유지; 누적1001/201 경계 |
| schema-vectors.json | strict envelope와 실패예상. 11.. contract hash/합성 block proof는 runtime 권위가 아님 |
| correction-state-hash.json | 완전한 EngineState 1회/2회 정정·WAL result bytes/hash·누적 재생과 변조 거절; 실제 체인/IO 증거 아님 |
| capacity.json | 실제 byte길이/hash와 고정 gas 모델·경계값 |

`manifest.json`은 protocol/v1·s1·s2 전체 상속 파일, S3 전체(자기 자신 제외), 고정 language lock을 합친 SHA256 manifest다. 집합 hash는 경로순으로 `sha256 + 두 공백 + repo-relative path + LF`를 연결해 SHA256한다. fee0/fee25 config hash는 각각 고정되며 합성 genesis도 분리했다. runtime genesis는 여기서 null/NOT_RUN이며 B/F가 실제 원본으로 별도 채워야 한다. 후보 manifest를 runtime 허가로 해석하지 않는다.

## 작성자 전용 재생성

```sh
cd chain
go run -mod=readonly ../protocol/s3/tools/crypto.go keys ../protocol/s3
cd ..
python3 protocol/s3/tools/build_schema.py
python3 protocol/s3/tools/build_vectors.py prepare
cd chain
go run -mod=readonly ../protocol/s3/tools/crypto.go sign ../protocol/s3
cd ..
python3 protocol/s3/tools/build_vectors.py envelopes
cd chain
go run -mod=readonly ../protocol/s3/tools/crypto.go sign ../protocol/s3
cd ..
python3 protocol/s3/tools/build_vectors.py finish
python3 protocol/s3/tools/build_state_hash_vectors.py
python3 protocol/s3/tools/check.py --seal
python3 protocol/s3/tools/check.py
```

암호 microbenchmark는 실행 환경에 따라 달라져 다시 생성하면 manifest도 바뀐다. 승인 후 재생성은 새로운 후보·재심사다. 생성 공개키/seed를 보드의 실제 키나 기존 S2 키로 바꾸지 않는다. 정산기/체인/엔진을 기동하는 코드는 이번 A에 없다.
