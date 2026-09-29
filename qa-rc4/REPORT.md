# S0-H rc4 독립 재현·착수 게이트

2026-09-29 · QA · [NUS-17](/NUS/issues/NUS-17)

**QA 판정: 승인된 S0 규약·공통 CI 재현 범위 PASS. 제품 T01~T16 모두 NOT_RUN.** CTO→CEO 네이티브 검토에 제출한다. 이후 기능 단계 실행, main 병합, 출시 또는 실자산 운영 승인이 아니다. 과거 rc2 420/413/7, rc3 759/754/5, fixed 783/782/1 FAIL과 기존 승인 이력은 변경하지 않는다.

## 입력과 재현

공유 root는 변경하지 않았다. 로컬 저장소에서 각각 `git clone --no-hardlinks . worktrees/NUS-17-rc4`, `git clone --no-hardlinks . worktrees/NUS-17-rc4-ci`로 새 clone을 만들었다. G 기반에는 QA 전용 qa/nus-17-rc4 브랜치를 생성했다. G의 prepare.py가 원본 Git 객체에서 C/D/E/F를 추출했고 새 바이너리·서명을 생성했다. 기존 evidence의 PASS 파일을 실행 증거로 재사용하지 않았다. evidence/local-g는 이번 run이 실제 갱신한 파일만 복사했으며, CI와 과거 783행은 별도 디렉터리에 분리했다.

| 입력 | full SHA |
|---|---|
| A rc4 | 57c187f5474e02c3f624667d3b8380268e13dd1a |
| B 공통 CI | 178aaaf1253ef8573ec7158e02291d92b40e3ea3 |
| C Go | b0283833a0d040366219b35de58a3150d3956e92 |
| D Rust | 7c7d0ca68b98f6c82fd297fa0dcd0a649194139d |
| E 정산 | f9b5bbf1e06a4a0c43bebc6d5bd081dc3b1892c2 |
| F TS | 5ff848415c3fda4e2285634950848c99824193be |
| G fresh checkout | a2fb8e83870e60ff11134a7d6c71c2605fed3ef5 |
| G Linux 시험 | 041fa5954f3fa950b100c7a505217d949165347b |

QA audit.py로 B/G의 지정 원본 A 44/C 24/D 42/E 13/F 43파일을 각 Git SHA와 byte 비교했다. B의 추가 골격 파일은 원본 일치 수에 포함하지 않는다. G의 마지막 증거 commit과 CI SHA 사이 harness/workflow 변경이 없음을 확인했다. E는 rc2 출처를 유지하고 rc4와 실제 소비하는 4파일의 호환성만 검증한다.

- contract SHA256: afa3471a2210a90cde0004b3219b231468bf0b8471e8068f8e16b096b3549d84
- vectors SHA256: bb1b437d23a365f1083e85353bf62bcef6517cda94288ff7e2d0b4cb5fccd55b
- B 입력 manifest: 05abeba166e00de2de12be35bd36c186a706cc9aada93ff666840927f2da7e9d
- B 실행 manifest: 70c09b2759d1d649ef829b8f90d46418846eba83451c324c6525330b9ead319f

protocol manifest의 43개 파일 hash를 재계산했다. 원본 manifest의 candidate/NOT_RUN 텍스트와 null runtime은 생성 당시 메타데이터로 보존한다. 현재 승인 근거는 [B CTO 인수](/NUS/issues/NUS-11#comment-0c2fb67d-57b3-45ca-b4d0-66a3df76e985)와 [G CEO 인수](/NUS/issues/NUS-16#comment-1cb7f26c-92c1-4ed5-afea-2122aefe5b74), 이번 별도 QA 실행 manifest다.

환경: Darwin arm64/macOS 15.6.1, Go 1.24.4, Rust 1.92.0, Node 24.21.0, Python 3.14.0, 실제 Chrome 154.0.8037.58. 고정 dependency lock으로 설치했다. G Go cache는 새 clone 내부이며 npm/Cargo는 설치된 사용자 캐시를 사용할 수 있다. 검증인 0, 실제 chain/DB/REST/WS 없음. seed는 벡터의 공개 test_seed_hex 및 SHA256("NUS-16 public synthetic "+producer)다. 정산 및 snapshot은 합성 상태다.

재현 명령:

1. G 새 clone에서 설치된 Rust bin을 PATH에 두고 CARGO/RUSTC/RUSTDOC 절대 경로를 설정한다. `python3 security/prepare.py`, `/usr/bin/time -l bash security/run.sh`.
2. G web에서 `EVIDENCE_DIR=../qa-rc4/evidence/browser npm run test:browser`.
3. B 새 clone에서 같은 toolchain으로 `/usr/bin/time -l make scaffold vectors`.
4. `gh run download 36635390972`, `gh run download 36635202163`로 기존 CI 원시 artifact를 내려받고 `python3 qa-rc4/audit.py`로 대조한다. audit는 B sibling clone과 evidence/ci 경로를 사용한다. 과거 783행은 고정 첨부 2b7afde8-13e4-4e26-b19a-de1419b5b7e1에서 추출한다.

## 실제 실행 판정

| 범위 | 결과 | 증거·제한 |
|---|---|---|
| G Go/Rust/TS build·시험 | PASS | security/run.sh exit 0, Go 338, Rust 18, TS 453 assertion/build |
| G differential | PASS | 969행 모두 기대 판정 일치; 언어별 323, 중복/누락 0. 기존 783개 ID/언어 보존 + 186 추가 |
| 실제 ML-DSA | PASS | 생성×검증 27, codec 27, 잘못된 domain/context 각각 27 거절 |
| rc4 전체 판정 | PASS | 원본 60개×3언어 및 독립 null/누락 epoch+false 6행. 새 서명으로 실제 인증/판정 함수 호출 |
| B 공통 CI 로컬 재현 | PASS | make scaffold vectors exit 0, Go 165/Rust 171/TS 171 oracle 및 rc4 60×3. gate 9/runtime 모의 11 |
| 정산 | PASS(모의) | 기존 16 + 독립 합성 adapter 20, E 생성/회귀 일치. 실제 원자 DB·chain commit 미실행 |
| 실제 Chrome | PASS(부분) | 453 checks, 키 생성/주문 서명/메모리 복구/390px UI, pageErrors 0, 외부 요청 0, overflow false |
| 기존 GitHub CI | PASS 대조 | G 36635390972 및 B push 36635202163 직접 조회·artifact 다운로드. 로컬 G 전체 969 JSON행 동일, B lane·rc4·manifest·경계 동일, browser suite 동일 |
| 제품·성능·분산 장애 | NOT_RUN | T01~T16, engine/finalized 처리량·p99·RPO/RTO 미측정 |

969행 중 872행은 actual 전체가 expected와 같고 97행은 기존 harness가 code/wire 등 지정 기대 필드를 비교한다. QA가 기대 필드의 존재와 값을 독립 확인했다. **969행 전체가 모든 부가 응답 필드까지 언어 간 같다는 뜻은 아니다.** rc4 60개 전체 decision은 원본 expected와 직접 일치한다. 로컬 대 Linux 전체 969행 동일성은 별도로 검증했다. 기존 B decision은 인증 주입 합성 정책 범위이며 rc4 실제 서명 사례와 구분한다.

QA audit 초안은 모든 행을 전체 응답 동일성으로 가정해 assertion이 실패했다. 원본 check 함수와 실제 결과를 읽어 지정 필드 비교 범위를 명시했고, 과거 evidence가 Git 추적 파일이 아니어서 고정 첨부에서 읽도록 수정했다. 이는 QA 감사 스크립트 수정이며 제품 코드·판정 기준 변경이 아니다.

## 결함·잔여 차단 범위

G-01~04, G-RC3-01/02, G-FIX-01은 rc4 회귀에서 PASS다. G-FIX-01의 snapshot ID/누락·null·모순 입력 기대/실제는 local-g/policy-inputs.json과 differential.json에 보존한다. H-01은 B manifest의 필수 입력과 실제 공통 CI 연결을 새 clone에서 재현해 해결 확인했다. 수정 담당은 기존 CTO/Chain/Exchange/Wallet/SRE이며 별도 중복 업무를 만들지 않는다. 이번 S0 범위의 새 미해결 차단 결함은 발견하지 않았다.

제품 차단 경계는 그대로다: ACK/원장/REST/WS/체인 NOT_CONNECTED, WAL replay·자산 보존·음수/중복 지급·출금/정산 양 순서·운영자 장애 중 직접 회수·분산 장애 NOT_RUN. 영속 백업은 미구현이다. 사용자 제공 snapshot을 신뢰하거나 합성 PASS를 durable ACK로 연결하면 안 된다. 실제 앱의 신뢰 context·원자 상태 소비는 후속 Chain/Exchange/Settlement 검증 대상이다.

## T01~T16 인수와 다음 시험계획

[원문 인수 기준](/NUS/issues/NUS-9#document-acceptance) d4f05eaa-dd46-487b-a520-00a1cd9cda0d, 측정 양식 8b1991b5-e355-40be-b916-c3047f9e0d86, M0 plan 43c89779-7be5-49f5-aed7-b4ae3de0a8a4가 현재 리비전임을 확인했다. 기존 작성·CEO 검토 이력과 설계서 인수 기록을 보존한다. TRACEABILITY.md의 원래 번호·필수 시나리오·통과 조건·담당자는 그대로이며 제품 판정 16개 모두 NOT_RUN이다. M0를 재개하지 않았다.

후속 실행 승인 뒤 Chain/SRE의 4검증인·genesis·두 테스트 계정과 Wallet/Settlement 실제 API를 고정한다. 동일 높이 C/T/U/bank/supply 대사, 사용자 권한, 음수·중복 지급 0을 검사한다. 주문/정산 연결 후 T01/02/03/05와 T04/06/07/09/15/16을 실행하며 barrier로 출금→정산과 정산→출금 두 순서를 강제한다. T05는 공통 장부 namespace 시험이며 제품 시장은 1개다.

T08은 독립 client ACK 집합을 보존해 WAL/outbox/fsync/복제 전후 중단과 실제 장애 영역 손실을 구분한다. T10은 fencing/구 리더 귀환, T13은 operator/relayer/RPC/sponsor 장애 중 독립 직접 회수, T14는 실제 화면의 잠정/확정/정정·확정 잔고/잠정 수취액 분리를 검증한다. 로컬 재시작이나 메모리 복구로 대신 통과시키지 않는다.

측정은 인수 양식을 사용해 engine ops/s와 finalized TX/s, offered/accepted·누락/중복·histogram·측정 구간·RPO/RTO를 분리한다. DEC-12 및 해당 실행 업무의 승인값 전에는 목표를 확정하지 않는다. 장기 200,000/3,000 수치를 v1 통과값이나 달성치로 쓰지 않는다.

## CTO 일정 재추정 입력

로컬 G 38.93s wall / user 57.17s / sys 10.92s / max RSS 441,286,656 bytes. B 54.46s / user 78.16s / sys 14.07s / RSS 435,470,336 bytes. 두 실행은 같은 호스트에서 병행했으므로 독립 전용 자원 벤치마크가 아니다. Chrome 2.30s / RSS 206,667,776 bytes. 이 수치는 빌드·시험 프로세스 측정이며 서비스 처리량이 아니다.

GitHub G job 21:45:41~21:47:21 UTC =100s; run 생성→job 시작 4s. B push 두 job은 76s/80s, 합계156s이며 run 생성→job 시작 3s/5s다. 초기 queue/스케줄 준비가 섞인 관측이며 순수 queue라고 단정하지 않는다. QA는 기존 CI를 다운로드·대조했으며 신규 CI를 요청했다고 주장하지 않는다.

B 제출 21:48:31.950→최종 CTO 승인 21:51:22.992 =171.042s; G 제출 21:50:40.137→최종 CEO 승인 21:56:04.061 =323.924s. 이는 제출 후 경과이고 순수 검토 대기/사람 작업시간이 아니다. 실제 수정 작업시간·에이전트 비용·총 달력 납기는 미측정이다. 이전 30~60분 조정 여유를 실제 소요로 바꾸지 않는다. CTO는 이번 자원·CI·검토 경과와 해결된 결함 범위를 반영한 재추정을 네이티브 review에 추가하고 CEO가 인수한다.
