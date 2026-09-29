# S0-H 독립 재현·착수 게이트 판정

2026-09-29 · QA · [NUS-17](/NUS/issues/NUS-17)

**S0 gate: FAIL. QA 검증 산출물은 완료하여 CTO→CEO 네이티브 검토에 제출한다.** Security의 G-01~04를 독립 checkout에서 그대로 재현했다. G가 done인 것은 부정 판정 산출물 인수이며 제품 PASS가 아니다. 수정 후 재시험은 NOT_RUN이다. 이후 기능 단계 구현·실자산 운영은 승인하거나 시작하지 않았다.

## 입력·독립성

공유 root의 branch를 변경하지 않고 `git clone --no-hardlinks . worktrees/NUS-17` 후 `qa/nus-17-s0-gate`를 만들었다. 기준은 Security 증거 commit `248b9529cfb297b8d8060ef9f923a32cab2a6e3a`; 실제 Linux CI harness commit은 `2be2347632909e1b6a417b3823220d5abcba4745`이다. 두 commit 사이 실행 코드는 동일하며 마지막 것은 문서/증거 추가다. `security/prepare.py`로 아래 Git object를 새로 추출했다. 다른 작업자의 checkout/cache를 시험 결과로 재사용하지 않았다. QA는 제품 구현을 수정하지 않았다.

|입력|고정 SHA|담당|
|---|---|---|
|B scaffold|cec18c78f41aea8c2c2935bf9ade6aa0fb34f3e9|SRE/CTO|
|C Go|b9c61ef6a0f5cc972b0290a39ac7510c05e3480b|Chain|
|D Rust|9bcbd3018a3ef9e8900368e91c983b38b658dc21|Exchange|
|E receipt/API|f9b5bbf1e06a4a0c43bebc6d5bd081dc3b1892c2|Settlement|
|F TS/browser|bb50bb9d93bdfdcd61f7b41a32e1149c5b4e1d89|Wallet|
|A rc2|889fda0c7181a696b4eb2a2649508c6192af8406|CTO|

C/D/E/F의 protocol 38파일(manifest 포함)은 같은 rc2와 byte-for-byte 일치한다. 별도 QA audit에서 manifest에 나열된 37파일의 SHA256을 재계산했다.

- contract aggregate: `daae05c8b3b94694d9f38122bf92a74f53b83197cd667d9ad93df545d55bc3dd`
- vectors aggregate: `60ee56ff3ff5a754965472cfb353dd1565072dbf0985907c369a0b78237f91e0`
- config: `7b12f8dffd4dfd07242331b975f0c440f5948074280c3a120c3232b3e674d13e`
- summary: `756d5e175f0c07b95de0529e3951d9ab67322e12f2698549ecba92c20b2cd357`

rc2 manifest의 상태는 여전히 candidate이며 runtime 필드는 비어 있다. QA 실행 manifest가 위 입력, dependency lock hash, 환경, 문서 리비전을 결합한다. 이것을 승인된 새 protocol manifest로 취급하지 않는다. CEO가 18:00 UTC에 원래 C/D/F 업무를 수정 반환한 댓글을 확인했으나, 이번 결과는 위 **수정 전** SHA에만 해당한다. 새 revision/수정 코드를 섞지 않았다.

## 실제 실행과 제한

환경 macOS 15.6.1 arm64, Go 1.24.4, Rust/Cargo 1.92.0, Node 24.21.0, Python 3.14.0, Chrome 154.0.8037.58. 잠금 파일 설치, 별도 빈 dependency cache, 공개 합성 seed만 사용했다. 검증인 0, 실제 chain/DB/REST/WS 연결 없음. seed는 공통 vectors의 test_seed_hex 및 `SHA256("NUS-16 public synthetic "+producer)`(Go/Rust/TS); 실제 개인 키가 아니다.

|검증|판정|직접 실행 증거·범위|
|---|---|---|
|B vector/runtime 검출기|PASS|`tests/test_vector_gate.py`, `tests/test_runtime.py`; 각각 6경계/11 모의 curl 시나리오를 포함한 unittest. 실제 runtime 기동 아님|
|B full vector manifest|FAIL/미연결|exit 2, contract_revision/vector path/hash·Go/Rust/TS vector command 6입력 누락. scaffold 빌드 성공은 기존 CI에서 확인; B 로컬 전체 scaffold 빌드는 재실행하지 않음|
|공통 protocol integrity/reference|PASS|`protocol/v1/tools/check.py`; 독립 파일 hash/기준선 대조. reference 산술을 제품 판정으로 승격하지 않음|
|Go 기존 test/runner build|PASS|`bash chain/test.sh`, 실제 ML-DSA; 새 바이너리 빌드|
|Rust 기존 test/runner build|PASS|12 integration tests 및 실제 IOC callback; `cargo test/build --locked`|
|TS Node/build|PASS|204 assertions, typecheck, standalone build|
|실제 브라우저|PASS(부분)|204 assertions, 키 생성·Order 서명·메모리 복구, 390px overflow 없음, pageErrors=0/externalRequests=0; 지속 백업·새 단말 복구·chain TX NOT_RUN|
|Settlement|PASS(모의)|기존 16 + 독립 20 synthetic adapter 조건; 실제 원자 DB/체인 commit NOT_RUN|
|실제 암호·codec 교차 검증|PASS|생산자 3×메시지 3×검증자 3=27 서명 검증, 27 encode/frame 일치, domain/context 변조 각각 27거절. 3 positive/35 negative 공통 암호 fixture 포함|
|정책 포함 전체 비교|FAIL|420 고유 (id,language), 413 일치/7 차이, `security/run.sh` exit 1. 새 QA audit는 예상 차이 집합·누락/중복·matrix 개수·manifest·CI 동일성 확인|
|기존 Linux repository CI|FAIL 확인|[G run 36607517804](https://github.com/nus-gang/cosmo-dex/actions/runs/36607517804) artifact 다운로드, 원시 로그 및 summary 동일. QA가 신규 CI를 실행했다고 주장하지 않음|
|제품 T01~T16/분산 장애/실자산|NOT_RUN|별도 추적표 참조. ACK replay·출금/정산 양 순서·자산 보존·직접 회수의 제품 PASS 없음|

첫 실행은 Go 시험 뒤 Rust default 미설정으로 중단(7.38초). 실패 로그를 attempt-1에 보존했다. 설치된 Rust를 명시한 재실행은 전체 비교까지 완료(17.98초)했고 정책 차이 exit 1이다. 환경 오류와 제품 판정 차이를 분리한다. Go의 verifier context 부정 fixture 3건은 제품 Verify 포트 대신 CIRCL raw API를 호출하므로 제품 context 포트 지원 증거가 아니다.

## 차단 결함과 재검증

모든 사례의 실제 요청은 `security/evidence/policy-inputs.json` 및 `review.py`, 기대/실제 전체는 `differential.json`; `bash security/run.sh`로 재현한다. G-03의 기대 OK는 기존 schema 해석이며 CTO의 새 정책 승인으로 취급하지 않는다.

|ID·영향|재현·기대 → 실제|수정 책임·재검증 기준|
|---|---|---|
|G-01 등록 key type / 통합 차단|동일 raw key, RegisteredKeyType=OTHER. 기대 ACCOUNT_KEY_MISMATCH; Rust OK, Go/TS 거절. Rust 입력 포트에 타입 누락|Exchange/CTO. OTHER·미등록·다른 bytes·정상 타입을 공통 최상위 경계로 전달. 수정 후 NOT_RUN|
|G-02 수수료 helper 차이|fee(0,0): 기대 0, Go/Rust FEE_GE_RECEIVE; fee(1000,10001): 기대 BPS_RANGE, TS FEE_GE_RECEIVE|Chain/Exchange/Wallet, CTO 계약. 0/양수 fee와 U128·bps 범위/error mapping 정렬; (0,0),(1,0),(1,25),(1000,25),(1000,10000),(1000,10001),U128_MAX 재시험. 수정 후 NOT_RUN|
|G-03 cap 의미 / 공통 판정 실패|q=p=100, active=0, cap=U32_MAX; Go/TS OK, Rust FEE_CAP|CTO 규약/Exchange 및 소비자. cap 0/25/10000/10001/U32_MAX와 active≤cap 명시. 보수적인 구현을 임의 완화하지 않음. 수정 후 NOT_RUN|
|G-04 성공 포트 의미 / 통합 차단|q=p=1, active=cap=25. 공통 acceptance 기대 FEE_GE_RECEIVE; Go/TS OK, Rust 거절. Go/TS 인증·부분 정책과 Rust 정책을 같은 접수 결과로 비교할 수 없음|CTO·Chain·Wallet·Exchange. 합성 snapshot 정책과 인증/실제 ACK를 구분한 adapter. 실제 앱/WAL/원장 구현은 이번 수정 범위 밖. 수정 후 NOT_RUN|
|H-01 B 공통 CI 연결 미완료 / 착수 게이트 차단|B 고정 manifest로 vector 명령 실행: 6입력 누락, exit 2; 기대는 공통 revision/hash와 3 runner 소비. B 초기 골격 자체의 승인 취소는 아님|CTO/SRE 통합 조정. 승인된 수정 hash/runner를 공통 manifest에 연결하고 새 checkout/CI 검증. G의 pinned harness는 현 실패 재현 증거이며 B full-vector PASS를 대체하지 않음. 미수정|

위 차이가 실자산 유출을 입증하지는 않는다. 자산·권한 불변식은 실제 앱 미구현으로 검증하지 못했다. 기존 원래 업무 반환/검토 경로를 사용하며 중복 수정 이슈를 만들지 않는다. CEO가 H-01의 원래 B/CTO 반환도 관리한다. 새로운 Security PASS와 QA 재시험 없이 이 FAIL을 해제할 수 없다.

## 다음 기능 단계 시험계획 — 실행 승인 아님

1. S0 수정은 CTO 공통 계약 결정→동일 hash의 C/D/F 수정→Security G 독립 재시험→QA 새 checkout 순서. 오류 코드와 인증/정책/ACK 결과 구분, 필수 ID 목록·기대값·최대/만료/키 타입을 CI manifest에 고정한다. 수정 전/후 SHA·판정은 별도 기록한다.
2. 이후 승인된 테스트 자산 단계에서 Chain/SRE는 고정 genesis·4검증인·테스트 계정 2개·초기화/조회/입출금 명령을 제공하고 Wallet/Settlement는 실제 서명·확정 조회 경로를 제공한다. QA는 동일 높이 C/T/U/bank/supply 대사, 음수/중복 출금/권한 위반 0을 검사한다. 구성·자원 미제공은 NOT_RUN.
3. 주문/정산 연결 후 원래 T01/02/03/05, T04/06/07/09/15/16 절차를 적용한다. barrier로 출금→정산과 정산→출금 두 순서를 강제하고 TX 인덱스를 증거로 남긴다. T05는 공통 장부 namespace 시험이며 제품은 DEVBASE/DEVQUOTE 1시장이다.
4. T08은 독립 client ACK 집합을 보존해 WAL/outbox/fsync/복제 경계 중단과 실제 장애 영역 손실을 구분한다. T10 fencing·구 리더 재가입, T13 operator/relayer/RPC/sponsor 중단 중 별도 경로 직접 회수, T14 잠정/확정/정정·P와 C 분리는 실제 환경/화면에서 검증한다. 로컬 restart와 메모리 키 복구만으로 통과시키지 않는다.
5. 측정은 NUS-9 양식을 그대로 사용한다. DEC-12 승인값 확인 전 engine ops/s·finalized TX/s·p99·RPO/RTO 통과값은 TBD. 원시 histogram, offered/accepted·오류/누락/중복, warm-up/steady/drain·시계 오차·resource profile을 기록한다. 장기 200,000/3,000 목표를 v1 기준으로 사용하지 않는다.

## CTO 일정 재추정에 제공하는 실측

- QA 전체 로컬 재현: 17.98s wall, `/usr/bin/time -l` user 36.88s / sys 6.72s, max RSS 416,186,368 bytes. 브라우저 1.57s wall, max RSS 207,831,040 bytes. 이는 시험/빌드 프로세스 자원이며 서비스 부하 성능·에이전트 비용이 아니다.
- G CI job: 17:48:18→17:49:14 UTC, 56s. B scaffold CI 36s, full-vector CI 29s(미연결 실패). 시작 전 queue와 에이전트 준비/검토 시간은 별도다.
- 댓글 시각상 제출→최종 승인 경과: B 2m23s; C 7m46s; D 7m59s; F 8m46s; G 6m10s. E는 첫 제출→변경 요청 3m49s, 재제출→승인 2m04s. 이는 검토 대기+실제 검토가 합쳐진 관측치이며 순수 queue time이나 사람 근무시간이 아니다.
- 실제 달력 납기·가용 에이전트 예산/비용·토큰 사용 총계는 여기서 검증하지 않았다. CTO 네이티브 review에서 위 자원/CI/검토 경과, G-01~04 및 H-01 재작업 범위를 근거로 다음 묶음의 크기·일정 재추정을 추가해야 한다. CEO가 그 결과와 이 부정 QA 판정을 인수해 사용자에게 보고한다. QA 제출 시점에 이 두 검토 단계는 미완료다.

원시 증거 ZIP과 QA 문서·PR을 현재 이슈에 연결한다. 이 보고서는 QA 자신이 수정한 코드의 유일한 승인 근거가 될 수 없으며 CTO→CEO 네이티브 검토를 따른다.
