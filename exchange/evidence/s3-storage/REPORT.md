# NUS-56 rc3 구현·검증 진행 보고서

2026-10-05 · Exchange · **NUS-67 allocator 설계·지원 gate 선행 / 전체 업무 미완료**

[NUS-65](/NUS/issues/NUS-65) 승인 head `51ab101ff5408b873cba101129b4c06962d56cc7` / tree `2a88bdf883434c52172ba59bc82017c27e962f5c`와 Security→QA 완료를 확인하고 전용 branch에 병합했다. main 기준선 `bd9e473196ac86fdedf655b2c93e6931f54faa83`, 이전 Exchange 후보 `b8cff9be1d2f951945b559496661321284f142fb`에서 이어지는 변경이다. 최종 Exchange code/tree는 전달 `candidate.json`과 commit work product를 따른다. 공통 protocol/manifest/lock은 승인본 그대로이며 main·공유 checkout·다른 담당 branch 변경0이다.

## 이번 구현

rc3의 모든 raw RPC/TxRaw를 exact bytes typed ref로 바꾸고, 저장·전이 참조·role/schema·길이·digest·중복 key를 검증한다. 원문 파일은 no-replace publish와 file/directory fsync를 거친다. descriptor는 같은 digest의 type 변경을 재시작 뒤에도 거절한다. Unicode canonical JSON/code point 상한과 supplementary12B escape를 구현했다.

승인 용량 oracle을 checked-u128로 이식했다. 누적 이력과 최대 미래 정정의 S/R/J/Q/B를 계산하며, B는 물리 할당 성공이 아닌 계산 상한이다. 원장·book/FIFO·fill ID·batch/attempt·cursor·CorrectionRecord/audit를 포함한 완전한 JournalRecord를 조립하고, 저장된 상태 대신 원 명령을 다시 실행해 full state/result/record를 대조한다. 확정 실패/불포함 attempt에 모순되는 뒤늦은 성공 receipt도 거절하도록 보완했다.

상세 API·내부 command kind·metadata 슬롯·원문 파일 순서·현재 제약은 `exchange/S3-STORAGE.md`에 있다. S3 runtime/REST/worker에 연결하지 않았으며 공개 durable ACK를 반환하는 API는 아직 없다.

## 결과

| 시험 | 판정 | 근거와 범위 |
|---|---|---|
| S3 후보·의미 재실행 | 8 PASS | 서명·0/25bps·불명 보류·정정·독립 survivor·8블록 부재·변조 재봉인 거절·terminal 충돌 |
| 새 rc3 저장·용량 | 8 PASS | Rust↔승인 Python 대조9형상, Unicode52형상, 원문/role/transitive refs/경로·권한/크기 경계 |
| S3 원장/그래프 | 7 PASS | 0/25bps·누적1000/1001 fills·200/201 orders 방향성 폐쇄 회귀 |
| S3 journal primitive | 16 PASS | helper1 포함. 기존6 crash경계×3회×2재생, frame16MiB/+1·원문 증거 검증 |
| S2 원장/journal/sequencer/snapshot | 68 PASS | 5/14/40/9. helper1 포함. 기존 기능 회귀 |
| clippy all-targets/all-features | PASS | -D warnings, 기존 Cargo.lock |
| 승인 S3 oracle | 13,248 PASS | 공통 명세/fixture 결과이며 제품 시험에 합산0 |
| 실제 전용 allocator·일반free0·경쟁 writer | NOT_RUN | NUS-67 설계·지원 gate 선행, reserve 삭제 방식으로 대체0 |
| 전체 semantic crash·외부 ACK 대사·단일 publisher | NOT_RUN/미완료 | storage primitive crash와 분리 |
| 실제 Chain/D/F/HTTP·독립 심사·main | NOT_RUN | 실제 genesis=null, 실자산0 |

Rust 항목107은 subprocess helper2를 포함한다. 자식 프로세스의 개별 stdout을 별도 성공 수에 더하지 않았다. 준비 중 변조 시험 하나가 이미0인 R을 다시0으로 쓰던 시험 오류를 발견해 실제 비영 R을 바꾸도록 고쳤고 최종 검사는 모두 PASS다. 개발 중 rustup HOME 쓰기 실패는 설치된 동일1.92.0 실행 파일·run 전용 cache 경로로 해결했다. 의존성/lock 변경은 없다.

정상0/25bps는 각각10명령 trace를3회 생성·두 번 재실행했다. 정정은17명령 trace를3회 생성·두 번 재실행했다. 전후 원장·주문/FIFO·fill ID·배치/시도·cursor·정정 revision 전체 bytes/hash가 같다. 정정 전 잠정 P 재사용과 timeout만으로 D/P 해제를 거절하고, 3개 의존 fill 정정 후 독립1개는 같은 ID/원서명으로 seq2/원VOID hash에 재구성했다. 해당 raw TX/서명/RPC는 합성 fixture이며 실제 체인 성공으로 해석하지 않는다.

3,145,728B 원문 regression은 digest `3a6bc3ad4851b52ca33f56f5fa632b23722ee6eaf066e4682cc7aac4978d31d2`를 그대로 disk에 보존·두 번 재개해 읽었다. 승인 schema형상 WAL99,671B와 full state/result hash를 확인했다. 이것은 실제 체인 proof도 실제 해당 정정 WAL append 시험도 아니다. 누적1000/1001 이력+pending2의 계산 B는 각각10,984,267,776B /11,028,652,032B이며 할당/비용/ACK 보장이 아니다. 모든1001 fills를 pending으로 둔 형태는 Jmax 초과로 거절된다.

## 남은 실행 경로

현재 호스트 APFS에서 rc3 `A(x)=ceil(x/4096)*4096+8192`가 실제 blocks·metadata·directory 갱신을 전용으로 보장한다는 근거를 확보하지 못했다. 실행 초기에 기본 Docker socket과 desktop-linux socket 모두 daemon 접속 실패를 관측했다. 이는 당시 관측이며 이후 상태를 추측하지 않는다.

기존 `correction.reserve`16MiB를 지운 뒤 일반 free에 append하는 구현은 다른 writer가 공간을 가져갈 수 있어 rc3를 충족하지 않는다. 이 경로는 과거 storage fault fixture로 명시했고 서비스 ACK에 사용하지 않는다.

[NUS-66](/NUS/issues/NUS-66)의 SRE 조사는 CTO→Security가 승인하여 완료했다. 승인 대상은 지원 미입증 조사 보고서다. 64MiB APFS image에서8MiB 사전 할당 소모는 available458 blocks 조건에서 성공했으나 free=0·8192B metadata 상한·경쟁 writer·실제 B/B−1·reservation ledger crash는 NOT_RUN이다. backend gate FAIL을 유지한다. [CTO 검토](/NUS/issues/NUS-66#document-cto-review)와 [Security 검토](/NUS/issues/NUS-66#document-security-review)는 제품 인수나 NUS-56 전체 완료를 승인하지 않았다.

CTO의 후속 방향에 따라 [NUS-67](/NUS/issues/NUS-67)을 생성했다. 전용 사전 할당 slot의 같은 inode 소모·metadata 상한·WAL/allocator commit 결합·지원 gate 제안을 initialPlan revision `a0b7f122-f34a-4c7f-9b10-897e7de7590f`로 전달하고 Security→QA 네이티브 review를 확인했다. CTO가 exact 설계와 지원 판단을 고정해야 한다. Exchange는 현재 계약을 임의 변경하지 않는다. 새 예산·유료 자원·공개 배포는 범위 밖이다. 완료된 NUS-66을 미해결 blocker로 사용한 이전 문구는 이 기록으로 정정한다.

Exchange는 그 승인 인계를 받아 실제 allocator·reservation ledger·단일 원자 publisher·bootstrap/외부 ACK 대사·전체 semantic crash matrix를 구현하고 검증해야 한다. 실제 receipt 연결은 이후 D/F가 수행한다. 이 업무의 최종 CTO→Security 검토는 아직 요청하지 않았고 완료를 선언하지 않는다.

## 재현 자료

`summary.json`에 승인 contract/config/vector/lock, compiler/host 버전·명령, source/raw SHA256, PASS/NOT_RUN과 합성 genesis를 기록했다. `validation.txt`, `clippy.txt`, `contract-oracle.txt`는 최종 원시 stdout/stderr다. `raw/`에는 bootstrap·0/25bps record trace·17명령 정정 trace·60개 exact raw/typed objects와 descriptor·원서명·TX/batch/fill ID·높이·예상 diff가 있다. `oracle_inputs.py`는 승인 Python 규범을 읽기 전용으로 재현하며 protocol 파일을 생성/재봉인하지 않는다.

보고서 artifact 및 patch·원시 증거 ZIP은 이 업무의 첨부와 artifact work product로 전달한다. 파일 경로만으로 인계하지 않는다.


검증 대상 구현 commit은 `17fb3a40dfb593dd18d15a2f317dbeb9c165eeed`, tree `c4cddc4c231d21df6dc5e5be27869c4a36df788f`다. NUS-66 완료/NUS-67 인계를 반영하는 후속 변경은 문서·상태 metadata만 수정하며 위107개 검사를 새로 실행했다고 주장하지 않는다. 기존 원시 시험 기록과 소스 SHA256은 보존한다.
