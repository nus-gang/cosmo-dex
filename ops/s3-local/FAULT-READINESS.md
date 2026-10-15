# L-R 장애 driver 마감 점검

2026-10-07 · SRE · checkpoint `629c30e` 이후 Apply crash CLI/거절 검증까지 반영한 작업 트리 소스 검사. 실제 서비스/START/RPC 실행 없음.

**현재 후보는 최종 runtime 심사 제출 준비가 끝나지 않았다.** 아래 작업은 L-R의 준비 범위이고, 실제 장애 실행과 DEV 판정은 승인 pin 이후 L-T에 남는다. 이 문서는 계약의 시험을 삭제하거나 더 작은 시험으로 대체하지 않는다.

## 구현된 경계

- [IO fault CLI](STORAGE-FAULT-RUN-CLI.md): `storage_fault_cli.py run-reviewed`는 독립 승인 조회·동일 capture·C validator·private child·READY·START 직전 조회를 조합한다. 기본 Seal, 명시적 `--fault-command Apply`를 지원하며 관측 저장 뒤 선택한 Worker 명령 한 번에만 hook을 설치한다. 일반 worker에 자동 주입하지 않는다.
- `runtime/storage_fault.rs`: 17개 hook 이름, 지정 방문 1..1024·전체 65536 상한. Generic IO/v2 또는 명시적 `--fault-errno ENOSPC|EDQUOT|EIO`/v3를 CLI→child→Worker→명령 원문/보고서에 결합한다. host 공간을 실제로 소진하지 않는다.
- [crash CLI](STORAGE-CRASH-RUN-CLI.md): `storage_crash_cli.py`의 READY 검사와 실행 명령은 별도 `--enable-storage-crash`를 요구한다. Seal/Apply를 지원하고 errno 혼합을 거절한다. `runtime/storage_crash.rs`는 선택한 기존 C hook에서 `_exit(86)`으로 Drop 없이 종료한다. 전원 손실 모의가 아니다.
- `storage_fault_report.py`와 `storage_crash_report.py`는 명령 원문/SHA·selector·reserved/final을 읽기 전용 검사한다. 실제 주입·명령 성공·replay·조직 승인을 인증하지 않는다. crash 예약/부분 final과 exit86은 UNKNOWN이며, 완전한 crash final은 RECORDED_NOT_REACHED다.
- 실제 C/L-D component 시험에서 Seal 및 COMMITTED/VOID Apply의 WAL 전/응답 직전 crash와 두 번 reopen을 확인했다. 실제 child READY/START 직전 거절도 확인했다. 합성 descriptor/pin/키/승인 reader의 근거이며 전체 F행 또는 실제 서비스 PASS가 아니다.
- `response_loss.py`: 지정된 사용자 직접 방송의 upstream 호출 뒤 응답을 버린다. settlement 방송의 header/JSON/chain commit 장벽과 같지 않다.
- 관리 session은 stop 확인 후 PID/port 및 worker C open 또는 Chain flock 관측을 연결한다. 자식 트리 목록 완전성·지속 배타성·전체 cleanup 보장은 여전히 false다.

## 필수 driver 공백

| 계약 항목 | 현재 부족한 준비 |
|---|---|
| F01 | Seal crash/IO 선택 가능: `before_wal`; 계약 NO_BATCH와 C 복구 거절 상태의 대조·3회 반복 근거 필요 |
| F02 | Seal crash 선택 가능: `evidence_complete`/`after_wal_sync`; immutable batch 객체·marker 사이의 exact 지점 선정/원문 증거 필요 |
| F03 | Seal crash 선택 가능: `after_commit`; 같은 batch replay·attempt/방송0의 실제 runner 대조 필요 |
| F04 | [전용 Attempt crash 경계](ATTEMPT-FAULT.md)를 `after_wal_sync`에 고정했다. TxRaw 객체+Attempt WAL fsync 뒤 marker 전 exact 명령을 예약하고 exit86, fee0/25 marker 불변·tail/transaction 보존·두 번 reopen `UNKNOWN_OR_INCOMPLETE_STORE`를 component 시험한다. 이는 잔존 transaction을 먼저 발견한 C의 fail-closed 코드이며 계약 결과 `UNKNOWN_TAIL_NO_NEW_ENVELOPE`를 유지한다. 실제 서비스/방송·3회 반복·DEV 판정은 NOT_RUN |
| F05 | 실제 C/L-D intent·영속 boundary 기록·crash/replay와 전용 child/READY/인증 run gate를 연결했다. fee0/25의 실제 Rust/C child에서 최종 승인 철회·capture 변경·stop·interrupt가 `START0`/evidence root0으로 거절되고 writer 재개방·두 번 replay를 확인했다. 실제 transport 호출과 DEV 판정은 NOT_RUN |
| F06 | [Settlement RPC 응답 경계](RPC-RESPONSE-FAULT.md)를 승인 Worker의 영속 `SUBMISSION_UNKNOWN` callback 안에 연결했다. exact TX/request hash·count1..3 뒤 header0 관측을 no-replace/fsync 보고서로 예약하고 오류/panic 후 재사용을 닫는다. fee0/25 실제 C/L-D 상태에서 accounts/batches 불변·writer2 거절·두 번 reopen을 확인했다. 소켓·실제 방송·crash·3회 반복·DEV 판정은 NOT_RUN |
| F07 | 같은 전용 경계가 완전한 HTTP header와 non-empty JSON prefix의 hash/길이를 기록하되 JSON을 해석하거나 Attempt를 종결하지 않는다. 실제 writer→읽기 전용 reader 교차 검증과 fee0/25 두 번 reopen을 확인했다. 소켓·실제 partial read·crash·3회 반복·DEV 판정은 NOT_RUN |
| F08 | [Chain commit→응답 경계](CHAIN-COMMIT-RESPONSE-FAULT.md)를 C가 영속한 `INCLUDED_SUCCESS/code0` Attempt 뒤에 연결했다. Receipt/Apply 전 no-replace/fsync 보고서를 남기고 fee0/25에서 응답 유실 오류 뒤 원 Attempt→원 Receipt 조회→Apply 1회·두 번 reopen/추가 효과0을 component 시험으로 확인한다. 실제 socket/RPC response loss·crash child·3회 반복·DEV 판정은 NOT_RUN |
| F09 | `runtime/receipt_apply.rs`가 승인 L-D `Receipt` 1회 반환 뒤 C commit/receipt 증가와 accounts/fills/chain_snapshot/corrections 불변을 확인하고 별도 fsync 보고서를 예약한다. `receipt_apply_report.py`는 exact 원문/SHA와 부분 기록을 읽기 전용으로 대조한다. 격리 child의 boundary `exit(86)` 뒤 예약-only `UNKNOWN`, Receipt 1개 보존, Apply0, 같은 home 두 번 reopen을 실제 C/L-D component 시험으로 확인했다. 전용 `receipt_apply_main.rs`와 private stage→READY→세 번째 승인→START 감독·인증 CLI를 연결했고 compile/순수 거절·정리 시험을 통과했다. 유효 START의 실제 관리 child crash/replay·3회 반복·DEV 판정은 NOT_RUN |
| F10 | 승인 COMMITTED Receipt 뒤 Apply의 `candidate_verified` exact hook에서 Generic IO를 한 번 주입하는 component 경계를 연결했다. fee0/25에서 보고서 원문/SHA·현재 commit을 결합하고, 오류 직후 reader는 `RECOVERY_REQUIRED`이되 state/commit은 이전 값 그대로이며 WAL/marker bytes도 변하지 않음을 확인한다. object transaction 예약 뒤 경계이므로 같은 home 두 번 reopen은 잔존 `transaction.dev`를 보존하고 `UNKNOWN_OR_INCOMPLETE_STORE`로 닫힌다. 실제 crash child·중간 동시 reader barrier·3회 반복·UI receipt 대조·DEV 판정은 NOT_RUN |
| F11 | 실제 Apply crash runner를 `after_wal_sync`에 연결했다. fee0/25×COMMITTED/VOID에서 exit86/reap, 이전 WAL prefix+새 tail 보존, marker 불변, `transaction.dev` 보존, 같은 home 두 번 reopen의 `UNKNOWN_OR_INCOMPLETE_STORE`, 보고서 원문 불변을 확인한다. 계약상 3회 반복·동시 reader/UI receipt 대조·DEV 판정은 NOT_RUN |
| F12 | [Apply response 경계](APPLY-RESPONSE-CRASH.md)를 `before_response`에 고정했다. fee0/25×COMMITTED/VOID에서 exit86/reap 후 완료된 WAL/marker, `transaction.dev` 제거, 기존 receipt ledger 원문 보존+정확히 1개의 Apply receipt, 두 번 reopen의 commit/state/ledger 불변과 추가 자산 효과0을 component 경계로 대조한다. 실제 HTTP/UI 응답 유실·3회 반복·DEV 판정은 NOT_RUN |
| F13 | [CLOSE Receipt→correction 경계](CLOSE-RECEIPT-CORRECTION-FAULT.md)를 승인 C/L-D VOID Receipt 저장 직후에 연결했다. 원 batch가 `CLOSING/ENGINE_APPLY_PENDING`이고 correction/replacement/Apply 및 경제 상태 변화가 0일 때만 no-replace/fsync 보고서를 예약한다. fee0/25 실제 component와 Python reader 교차 검증, 같은 home 두 번 reopen을 통과했다. crash child·3회 반복·인증 CLI·DEV 판정은 NOT_RUN |
| F14 | 승인 C3802bf0의 CorrectionPhase::Prepare hook 통합 완료. SRE Prepare selector/명령 원문/영속 보고서/reader/인증 CLI 및 실제 child START 직전 거절 연결 완료. 실제 실행/DEV 판정은 NOT_RUN; SemanticReplay를 F14로 대체하지 않음 |
| F15 | [VOID correction Apply WAL 경계](VOID-CORRECTION-WAL-CRASH.md)를 `after_wal_sync`에 고정했다. WAL tail의 실제 `CORRECTION` record/result/state를 decode해 correction closure·dependencies·원 VOID receipt/attempt refs를 대조하고 marker 불변·transaction 보존·두 번 reopen fail-closed를 component 시험한다. 실제 관리 child·3회 반복·DEV 판정은 NOT_RUN |
| F16 | [VOID correction publish 경계](VOID-CORRECTION-PUBLISH-CRASH.md)를 `before_publish`에 고정했다. durable WAL/marker 뒤 Arc publish 전 exit86, 재시작이 WAL의 전체 state(accounts/holds/order/FIFO/fills/dependencies/correction/cursor)와 원 receipt ledger prefix+Apply entry를 정확히 복원하는지 대조한다. 실제 동시 reader/UI·3회 반복·DEV 판정은 NOT_RUN |
| F17 | [승인된 F17/DEV09 해석](F17-DEV09-HANDOFF.md)에 따라 원 snapshot temp fsync→rename / OLD_VALID_SNAPSHOT_PLUS_WAL / NOT_RUN 유지. 개발 marker/Arc를 F17 PASS 또는 N/A로 대체하지 않음. 현 개발 경로에 이름을 맞추기 위한 파일 checkpoint 추가 불필요; DEV09 각 실제 경계의 준비·검증은 그대로 필수 |
| F18 | 단일 writer·오류/원문 검사의 부분 근거 존재; 17변형 전체 runner 및 원 개발 receipt 대조 미완료 |

모든 행의 실제 결과는 NOT_RUN이다. 공통 `file_sync`/`before_publish` 이름이 존재하는 것만으로 특정 경제 명령·정확한 crash 위치에 도달했다고 판정하지 않는다. 3회 반복·2회 replay와 원 개발 receipt 대조도 별도 runner/근거가 필요하다. F18의 17개 variant 원문은 동봉 `audit.json`에 보존한다. 외부 Linux/실제 host 공간 소진/표준 ACK 조건은 이월 상태를 유지한다.

DEV10은 ENOSPC/EDQUOT/EIO·부분 write·cap 거절을 요구한다. 현재 범용 IO 오류와 실제 C replay 시험은 부분 근거다. Seal/Apply의 실제 C 명령·errno 보고서 및 CLI 연결은 준비됐지만 명령별 전체 지점·부분 write·cap·접수 ledger 대조와 실제 DEV 실행은 남아 있다. host 디스크를 채우는 방식은 사용하지 않는다.

## 최종 후보 봉인 전 SRE 작업 순서

1. 기존 승인 C hook으로 가능한 crash/명령 선택 범위를 위 표에 맞춰 구현한다. 순수 컴파일·선택 거절/종료 시험까지만 이 업무에서 실행한다. 경제 상태·proof·C API 변경이 필요하면 원 구현 업무의 수정→CTO→Security를 먼저 받는다.
2. 방송/header/JSON/receipt/Apply와 correction의 명시적 장벽·증거·단일 실행 경로를 연결한다. 타이밍 sleep이나 임의 PID kill로 정확한 경계를 주장하지 않는다. 준비할 수 없는 필수 경계는 원 담당에게 구체 API 공백으로 반환한다.
3. fee0/25 입력 생성·네 home 게시·관리 등록 packet·topology·stop/증거 보존을 하나의 실행 안내로 대조한다. runtime host command 등록은 권한 있는 Paperclip 경로로 인계하며 agent 제한을 우회하지 않는다.
4. 통합 후보를 Git에 고정하고 최신 component 승인 출처와 실제 파일 일치를 확인한다. `manifest.py`의 HEADS는 `component_sources.CANDIDATES`의 C `3802bf0`, L-D `46546d3`, L-E `720163e`를 재사용한다. exact bytes/mode/누락·계약 원문·최종 재조회 gate가 있다. C/L-D 교차 병합 재현은 승인 예외가 아니므로 COMPONENT_SOURCE_RECONCILIATION_REQUIRED를 유지하고 exact 병합의 독립 검토 출처를 결합해야 한다. checkpoint 이후 미커밋 변경도 최종 후보에 포함해야 한다.
5. 실제 offline/locked build·기본 feature off·거절 시험 결과와 binary/web/launcher 전체 SHA를 다섯 descriptor에 묶는다. 합성 test pin·source SHA를 승인 pin으로 쓰지 않는다. CEO/CTO 원문 독립 승인 및 동일 후보 CTO→Security 후에만 L-T에 인계한다.

## 이번 검사와 한계

초기 `audit.json`은 당시 9파일 기준의 과거 기록으로 보존한다. 이번 `fault-readiness-refresh` artifact는 현재 근거 소스의 SHA256과 계약 원문을 별도로 보존한다. 위 hook 매핑은 소스 기준 준비 후보이며 계약 충족 판정이 아니다. 문서 정정으로 runtime manifest나 기동 허가를 만들지 않는다. 신규 제품 동작 변경·Rust/Go build/실제 장애 실행은 없다.

G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false, €0, DEV NOT_RUN을 유지한다. 미완료 작업을 L-T의 실행 책임으로 넘겨 L-R 완료로 처리하지 않는다.
