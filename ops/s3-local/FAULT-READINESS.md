# L-R 장애 driver 마감 점검

2026-10-07 · SRE · 소스 검사 결과. 실제 서비스/START/RPC 실행 없음.

**현재 후보는 최종 runtime 심사 제출 준비가 끝나지 않았다.** 아래 작업은 L-R의 준비 범위이고, 실제 장애 실행과 DEV 판정은 승인 pin 이후 L-T에 남는다. 이 문서는 계약의 시험을 삭제하거나 더 작은 시험으로 대체하지 않는다.

## 구현된 경계

- `storage_fault_cli.py run-reviewed`: 독립 승인 조회·동일 capture·C validator·private fault executable·READY·START 직전 조회를 조합한다. 별도 fault child는 신뢰 관측을 한 번 저장하고 Worker Seal 한 명령에만 hook을 설치한다. 일반 worker에 자동 주입하지 않는다.
- `runtime/storage_fault.rs`: 17개 hook 이름, 지정 방문 1..1024·전체 65536 상한, `Error::Io(Error::other("SRE_INJECTED_IO"))` 한 번. 내부 `with_io_fault` API는 ENOSPC/EDQUOT/EIO를 선택할 수 있다. 아직 CLI/명령 envelope/보고서 reader에는 미연결이며 v2 기록 경로는 errno mode를 IO 전에 거절한다. **프로세스 crash 선택기는 없다.** `partial_wal`은 C에서 WAL 중간 write 뒤의 hook이지만 현재 adapter는 IO 오류를 반환한다.
- `storage_fault_report.py`: 원문/SHA와 reserved/final 일관성을 읽기 전용 검사한다. 실제 주입·Seal 성공·replay·조직 승인 인증을 하지 않는다. 부분 final은 UNKNOWN이다.
- `response_loss.py`: 지정된 사용자 직접 방송의 upstream 호출 뒤 응답을 버린다. settlement 방송의 header/JSON/chain commit 장벽과 같지 않다.
- 관리 session은 stop 확인 후 PID/port 및 worker C open 또는 Chain flock 관측을 연결한다. 자식 트리 목록 완전성·지속 배타성·전체 cleanup 보장은 여전히 false다.

## 필수 driver 공백

| 계약 항목 | 현재 부족한 준비 |
|---|---|
| F01 | Seal IO only; no deterministic crash mode |
| F02 | Seal IO only; no deterministic crash mode |
| F03 | Seal IO only; no deterministic crash mode |
| F04 | No attempt-command fault driver |
| F05 | No persisted-intent/send barrier driver |
| F06 | Direct browser response loss is not settlement before-headers barrier |
| F07 | No settlement partial-JSON barrier driver |
| F08 | No chain-commit-confirmed response-loss barrier |
| F09 | No receipt-command/apply barrier driver |
| F10 | No Apply-command fault driver |
| F11 | No Apply-command fault driver |
| F12 | No Apply-command fault driver |
| F13 | No CLOSE-receipt/correction barrier driver |
| F14 | No correction-closure barrier driver |
| F15 | No correction-command fault driver |
| F16 | No correction-command fault driver |
| F17 | Snapshot persistence boundary requires component/API mapping |
| F18 | Partial checks only; variant inventory and driver incomplete |

모든 행의 실제 결과는 NOT_RUN이다. 공통 `file_sync`/`before_publish` 이름이 존재하는 것만으로 특정 경제 명령·정확한 crash 위치에 도달했다고 판정하지 않는다. 3회 반복·2회 replay와 원 개발 receipt 대조도 별도 runner/근거가 필요하다. F18의 17개 variant 원문은 동봉 `audit.json`에 보존한다. 외부 Linux/실제 host 공간 소진/표준 ACK 조건은 이월 상태를 유지한다.

DEV10은 ENOSPC/EDQUOT/EIO·부분 write·cap 거절을 요구한다. 현재 범용 IO 오류와 실제 C replay 시험은 부분 근거다. 내부 errno 선택의 단위시험은 통과했으나 실제 C 명령·CLI/보고서 연결 및 명령별 선택·결과 수집을 준비해야 하며 host 디스크를 채우는 방식은 사용하지 않는다.

## 최종 후보 봉인 전 SRE 작업 순서

1. 기존 승인 C hook으로 가능한 crash/명령 선택 범위를 위 표에 맞춰 구현한다. 순수 컴파일·선택 거절/종료 시험까지만 이 업무에서 실행한다. 경제 상태·proof·C API 변경이 필요하면 원 구현 업무의 수정→CTO→Security를 먼저 받는다.
2. 방송/header/JSON/receipt/Apply와 correction의 명시적 장벽·증거·단일 실행 경로를 연결한다. 타이밍 sleep이나 임의 PID kill로 정확한 경계를 주장하지 않는다. 준비할 수 없는 필수 경계는 원 담당에게 구체 API 공백으로 반환한다.
3. fee0/25 입력 생성·네 home 게시·관리 등록 packet·topology·stop/증거 보존을 하나의 실행 안내로 대조한다. runtime host command 등록은 권한 있는 Paperclip 경로로 인계하며 agent 제한을 우회하지 않는다.
4. 통합 후보를 Git에 고정하고 최신 component 승인 출처와 실제 파일 일치를 확인한다. 현재 `manifest.py`의 HEADS는 초기 C/L-D/L-E 값이다. 후속 승인 C `20c0cd9`, L-D `46546d3`, L-E `720163e`의 full head/tree·판정/revision과 포함 관계를 새로 검증해야 한다. 초기 ancestor 존재만으로 최신 API 승인을 증명하지 않는다.
5. 실제 offline/locked build·기본 feature off·거절 시험 결과와 binary/web/launcher 전체 SHA를 다섯 descriptor에 묶는다. 합성 test pin·source SHA를 승인 pin으로 쓰지 않는다. CEO/CTO 원문 독립 승인 및 동일 후보 CTO→Security 후에만 L-T에 인계한다.

## 이번 검사와 한계

`audit.json`은 검사한 원본 9파일 SHA256, 계약 F01~F18 18행, F18 17 variant, 반복/재생 수를 보존한다. 이 파일은 runtime manifest 입력이 아니며 기동 허가로 사용하지 않는다. 보고서 reader 기존 5시험을 재실행했다. 신규 제품 동작 변경·Rust/Go build/실제 장애 실행은 없다.

G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false, €0, DEV NOT_RUN을 유지한다. 미완료 작업을 L-T의 실행 책임으로 넘겨 L-R 완료로 처리하지 않는다.
