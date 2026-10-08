# ALLOC-67-06 — 현재 Mac 전용 S3 개발 시연 계약 후보

2026-10-05 · CTO · NUS-67 · **DRAFT / 기본 비활성 / 구현·제품 인수 아님**

이 후보는 외부 Linux를 이월하고 현재 Mac만 사용하라는 사용자 방향을 위한 전문 심사 입력이다. `G00=FAIL_UNPROVEN / supported_backend_allowlist=[] / durable ACK=CLOSED`는 유지한다. 원 rc3 내구성 계약을 삭제하거나 APFS를 지원 backend에 넣지 않는다. 새 공급자·계정·설치·유료 자원·실자산·main 반영을 허용하지 않는다.

## 1. 적용 범위와 활성화 경계

후보 ID는 `s3-dev-local-v1`, envelope version은 `s3-dev-local/1`이다. 이 문서·profile·acceptance·manifest는 루트 `proposals/`에만 두며 활성 `protocol/s3` 파일, language lock, S2 경로를 수정하지 않는다. 현재 S3 runtime/REST/worker의 완성된 개발 실행 경로는 없다. 문서에 적은 build feature와 launcher 옵션은 구현 요구이며 지금 실행 가능한 CLI라고 주장하지 않는다.

활성화에는 (a) exact 후보 head/tree/manifest의 Security→QA 승인, (b) 부모가 기존 담당 업무의 개발 전용 범위·선행과 전문 검토를 기록, (c) Exchange/Chain/Settlement/Wallet/SRE의 구현·검토된 통합 manifest가 모두 필요하다. 승인 계약 없이 구현자가 임의 fallback을 붙이지 않는다. 이번 제출은 이 세 조건을 충족했다고 표시하지 않는다. NUS-68→67→56의 표준 경로 blocker는 그대로 둔다.

실행 프로그램은 표준 binary와 분리한 `nus-s3-local-demo` 이름 및 기본 비활성 `dev-local-demo` build feature를 요구한다. 명시적인 `--local-demo-profile <검토된 effective profile 파일>`과 `--acknowledge-unproven-space`가 동시에 없으면 개발 서비스 시작을 거절한다. 표준 서비스에 이 옵션을 붙여 gate를 건너뛰는 동작은 없다. 환경변수 한 개·OS 감지·디스크 여유 조회·G00 실패로 자동 전환하지 않는다. CI 기본·표준 launcher는 개발 feature를 켜지 않는다.

## 2. 별도 home·genesis·식별자

`effective-profile-fee0.json`/`effective-profile-fee25.json`은 승인 rc3 profile의 정확한 복사에서 `profile`, `durability`, `runtime_root` 세 key만 바꾼다. 원 wire/schema의 경제 객체 Context는 `s3/3`, chain ID는 `nus-s3-dev-1`을 유지한다. 이 값의 재사용은 전체 저장 서비스가 rc3를 충족한다는 주장이 아니다. 전송·저장 바깥 envelope `s3-dev-local/1`과 아래 hash/namespace가 개발 경로를 별도로 식별한다.

새 runtime root는 `.runtime/s3-dev-local-v1/<run_uuid>/fee0/` 또는 `fee25/`이며 기존 `.runtime/s3/`, S1/S2, 이전 genesis/home/journal을 받지 않는다. 최초 생성은 빈 새 directory·0700·단일 writer lock 아래에서만 허용하고, 경로의 symlink/alias·기존 inode 공유·root 밖 접근을 거절한다. 같은 home 재시작은 최초 생성한 guard와 실제 canonical path/inode·Context가 모두 일치할 때만 허용한다. 개발 디렉터리의 hardlink·symlink로 표준 파일을 가리키는 것을 허용하지 않는다.

각 fee profile마다 새 4검증인 genesis와 새 임시 ML-DSA 사용자·운영자 계정을 생성한다. 공개 고정 fixture key는 실제 4검증인 시연에서 사용하지 않는다. DEVBASE/DEVQUOTE/DEVGAS 합성 자산만 허용한다. S1/S2 key·HELD_S2 outbox·기존 S3 기록 import0, 표준 모드로의 승격/자동 변환0이다.

`profile.guard.json`은 canonical JSON으로 `envelope_version`, `profile_id`, `candidate_manifest_sha256`, `runtime_manifest_sha256`, `effective_profile_sha256`, `run_uuid`, `fee_profile`, 전체 `Context`를 정확히 포함한다. 추가/누락/중복 key, hash 불일치, 다른 fee/run/genesis, 기존 home에 guard만 새로 만드는 행위를 거절한다. guard를 file fsync→no-replace publish→parent directory fsync한 후에만 store를 초기화한다. guard는 수정하지 않으며 모든 engine/worker/REST/chain adapter/UI capability가 같은 값을 사용한다. 동일 hash만 보고 파일·원문 검증을 생략하지 않는다.

Context.contract_hash는 최종 개발 runtime manifest의 `contract_sha256`이고 config_hash는 해당 effective profile의 exact file SHA256이다. 최종 manifest는 승인 rc3 contract/config/vector/lock 전체와 이 후보의 고정 파일, 심사를 통과한 component head/tree·선언된 구현 설정을 포함해 기존 방식 `SHA256(sorted sha256 + two spaces + repo-relative path + LF)`로 집계한다. manifest 자신·genesis·비밀키·실행 결과는 이 집합에서 제외해 hash 순환을 금지한다. genesis는 그 두 hash를 app_state에 넣고 만든 exact bytes를 별도로 SHA256하여 Context.genesis_hash로 사용한다. 현재 후보 manifest는 runtime manifest가 아니다. 실제 genesis/hash/서명·TX/height는 구현 전 NOT_RUN이다.

개발 WAL magic은 `S3D1`, 파일명은 `journal.dev.wal`, marker는 `commit.dev.json`이다. header layout72B, payload ceiling16MiB, frame hash 계산은 rc3와 같고 magic4B만 다르다. header magic을 포함한 full frame을 해시한다. 표준 S3W1과 상호 개방/변환을 거절한다. schema-valid 경제 record를 다시 해시해 표준 WAL로 승격하지 않는다. raw object SHA/length/media·`.ref`·no-replace와 원문 보존 규칙은 그대로다. NUS-69 arena는 이 프로필에서 활성화하지 않는다.

## 3. 개발 접수 응답과 자산 의미

개발 API는 loopback의 `/dev-local/v1/` 아래만 노출하고 기존 signed 요청·인증·origin·계정 격리를 유지한다. 포트는 SRE가 기존 점유와 충돌하지 않는 실행 manifest에 고정하며 임의 공개 bind나 proxy/tunnel은 없다. 표준 endpoint·capability에 durable ACK 지원을 광고하지 않는다.

변경 응답은 envelope의 required key `envelope_version`, `profile_id`, `context`, `development_receipt`, `durable_ack`, `storage_assurance`, `command_result`를 가진다. 값은 각각 `s3-dev-local/1`, 선택 profile ID, 검증된 Context, `LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE`, false, `UNPROVEN_HOST_SPACE`, rc3 CommandResult다. 이 응답은 현재 fsync/marker 완료 관측이며 미래 drain·전원 상실·host ENOSPC 생존 보장이 아니다. 응답이 유실되면 같은 signed request ID로 조회하고 추가 효과 없이 원 결과를 반환한다. 독립 client receipt ledger에는 이 envelope 원문과 seq/hash를 보존하고 **ACK 집합**이라고 부르지 않는다.

UI에는 연결 중인 개발 profile과 “로컬 개발 시연 · 저장공간 고갈 시 복구 보장 미검증”을 표시한다. 거래 상태는 잠정/불명/COMMITTED/정정을 계속 구분한다. 개발 접수 완료나 CheckTx·RPC 성공·timeout이 확정 잔고를 만들지 않는다. 확정 수취와 일반 출금 가능은 실제 chain receipt와 같은 H의 C·engine revision·D/P 대사로만 정한다. `A=C−R−D`, `P 재사용0`, 한 미확정 batch, 서명·정수·fee·전체 rollback·추가 지급0 규칙은 동일하다. 출금은 사용자 직접 서명이며 결과 불명 상태에서 추가 자동 출금0이다.

## 4. 저장 동작과 약해진 보장의 정확한 범위

일반 APFS 파일을 사용하되 exact evidence write/fsync→검증→no-replace publish/dir fsync→후보·semantic replay 검증→WAL fsync→marker temp fsync/rename/dir fsync→단일 snapshot/result 공개 순서는 유지한다. worker도 exact Batch/TxRaw·attempt를 방송 전에 영속화한다. 현재 NUS-56의 저수준 Journal과 in-memory Candidate를 바로 서비스로 연결하는 것으로 이 요구가 구현됐다고 간주하지 않는다. 완전한 한 writer/publisher·bootstrap·cursor·receipt ledger 연결은 기존 소유자의 작업이다.

표준 profile의 **미래 B에 대한 물리·metadata 독점 예약 및 available=0에서 이미 ACK된 집합 drain 보장**을 이 개발 profile에는 제공하지 않는다. 개발 응답은 durable ACK가 아니며 해당 보장을 인수하는 새 이름도 아니다. rc3의 raw16MiB/Tx139264/typed262144, payload16MiB, Q·metadata96/tx5/raw80, 이력·정수·Unicode 상한은 유지한다. certificate는 상한 계산/새 입력 거절에 쓰며 확보한 물리 credit으로 표시하지 않는다. 일반 free-space 조회, sparse reserve 또는 reserve 삭제로 전용 예약을 주장하지 않는다.

ENOSPC/EDQUOT/EIO/부분 write/fsync 실패·미상 tail·hash 불일치는 즉시 `RECOVERY_REQUIRED`: 신규 접수·방송·자동 정정·출금 준비 승인을 닫고 기존 원문·WAL·marker·시도·D/P를 보존한다. 오류 발생 전 실제 chain에 반영된 자산은 되돌리지 않는다. 자동 truncate/delete/reseed·D/P 해제·새 Batch ID 발급·새 TX 봉투 재시도0. 남은 공간이 생겼다는 이유만으로 복구 성공으로 바꾸지 않는다. 운영자 증거 검토 후 complete prefix와 원 체인 상태를 대사하며 근거가 없으면 닫힌 상태를 유지한다. 저장장치 손실 후 자료 복구나 drain 완료를 약속하지 않는다.

모의 IO error/byte budget은 명령·주입 위치·예상 errno·보존 증거를 기록하는 독립 local fault 결과다. 실제 host ENOSPC, G01~G08, APFS 물리 보장, 전원 상실로 합산하지 않는다. Mac disk 채우기·image/mount·경쟁 free-space 소모·VM 시작은 이 후보의 실행 범위가 아니다.

## 5. 이미 확인한 통합 차이와 기존 소유자

승인 Chain f44d511의 `S3Context`/query driver는 service_schema `s3/1`이고 Exchange83a020a는 `s3/3`이다. 따라서 기존 두 후보를 붙이면 곧바로 동작한다는 명령을 제공하지 않는다. NUS-55/Chain은 승인된 payload Context s3/3와 새 runtime hash를 명시적으로 지원하도록 변경·전문 검토해야 한다. `keeper/s3_store.go`의 Context는 `S3ContractHash`/fee별 config 상수를 사용하므로 genesis에 다른 hash를 적는 것만으로 호환되지 않는다. 초기화·query·restart의 hash 검사를 함께 다루고 기존 표준 hash 검사를 느슨하게 만들지 않는다. 사용자/BatchV2 wire byte layout은 유지하되 새 genesis에 결합한 서명 fixture는 새로 만들어 검증한다. 구 Context 자동 수락0이다.

| 기존 업무 | 첫 담당 행동 | 후속 전문 검토 |
|---|---|---|
| NUS-54 CTO 계약 | 본 exact 후보를 기존 Security→QA 네이티브 심사에 연결할 범위를 부모가 기록 | Security→QA, 지원 G00 심사와 분리 |
| NUS-55 Chain | s3/1↔s3/3 query 불일치·guard/manifest와 재시작 Context를 고정한 호환성 시험부터 준비 | CTO→Security |
| NUS-56 Exchange | 별도 build/store header·single publisher·semantic bootstrap·개발 receipt를 exact 계약에 연결 | CTO→Security; 표준 G00 blocker 유지 |
| NUS-57 Settlement | 미구현 S3 연결점을 inventory하고 profile/receipt gate를 검증한 mock adapter부터 준비 | B/C 승인 후 실제 연결, CTO→Security |
| NUS-58 Wallet | 별도 capability/개발 접수/확정 수취·불명/출금 보류 표시 fixture | D 선행, CTO→Security |
| NUS-59 SRE | 현재 Mac의 새 root/포트·프로세스 목록·기동/종료·fresh genesis manifest 정의 | B/C/D/E 승인 후 통합, CTO |
| NUS-60 Security / NUS-63 QA | acceptance.json의 실제 결과·결함 주입 독립 재현 | 기존 G/J 및 main 인수 조건 유지 |
| NUS-68 SRE | 외부 G00 proof·ENOSPC·reservation drain은 이월 유지 | 사용자 외부 방향 변경 뒤에만 actual tuple/범위 재확정 |

현재는 native review 입력 제출 단계다. 본 후보 때문에 다른 담당의 blocker를 삭제하거나 완료 이슈를 조용히 재개하지 않는다. 부모는 새 업무를 만들지 않고 기존 NUS-54 계약 후속을 네이티브 Security→QA에 배치하는 경로를 먼저 확정한다. NUS-67 자체의 최종 지원 심사는 NUS-68 뒤에 유지한다.

## 6. 검증·인수 분리

`acceptance.json`은 구현 후 충족할 개발 profile 조건이며 전 항목 NOT_RUN이다. 순수 모델·기존 SDK/Rust 작성자 시험을 이 표의 실제 통합 PASS로 승격하지 않는다. DEV 통합 보고서는 profile, exact heads/trees·manifest·genesis, raw input/output, HTTP receipt·chain H/TX/index·batch/fill·전후 C/R/D/P·client receipt ledger·두 번 replay diff·exit를 남긴다. 합성 값은 명시하고 실제 browser·4검증인·main 미실행은 NOT_RUN이다.

일차 시연은 별도 0bps와25bps genesis의 기존 2사용자 예치→서명 주문→잔량 취소→COMMITTED→수취 자산 사용자 출금이다. 정상/실패·중복·두 출금 경합 순서·unknown·정정·writer2·재시작 시험과 함께 자산 보존을 검증한다. DEV PASS라도 원 S3-AT08의 물리 예약/ACK 보장이나 S3 전체/J/main 인수 완료가 아니다. 이월 G00과 표준 게이트를 보고서 첫머리에 계속 표시한다.
