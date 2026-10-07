# 독립 승인 조회 — 구현 중

`paperclip_reader.Reader.from_environment()`는 현재 run의 API URL/인증을
사용하는 읽기 전용 로컬 Paperclip reader다. localhost/127.0.0.1/::1과
명시적 포트, 이 업무 및 두 승인 문서 GET만 허용한다. 프록시·redirect·
파일 fallback·응답 로그를 사용하지 않는다. 인증 토큰을 worker/웹에
전달하거나 배포 파일에 저장하지 않는다. 이 reader는 L-R 검증 과정용이며
L-T의 새 run 인증/관리 runtime 연결은 아직 구성되지 않았다.

`review_documents.inspect(reader, subject_bytes, revisions)`는 CEO와 CTO의
서로 다른 문서 `runtime-ceo-approval` / `runtime-cto-approval`을 두 차례
최신 조회한다. 회사·업무·키·문서 UUID·리비전·원 작성자·최종 수정자를
검사한다. 본문은 `approval_body(subject_bytes, role)`가 만드는 canonical
JSON 원문(문서 format=markdown)과 정확히 일치해야 한다. 이 형식은 실행
후보 원문(manifest/5 descriptor의 base64 포함), build·binary 목록 및 그
subject 원문 SHA256를 승인 출처에 결합하기 위한 제안이다. 본문 생성은
승인이 아니다. SRE는 이 두 승인 문서를 대신 게시하지 않는다.

리비전 변경, 철회 본문, 다른 후보, 역할/작성자 교체, API 오류는 거절한다.
정확한 이전 리비전을 가져와 최신 철회를 무시하는 경로는 없다. 검증 뒤
다른 실행 시점에도 재조회가 필요하며 이 결과는 재사용 가능한 permit이
아니다. `native_review.inspect`의 CTO→Security 완료 검사와 독립 문서 검사
모두 현재 `approval_verified=false`를 반환한다. 최종 판정의 exact 후보
결합·launcher gate 합성·TOCTOU 처리·독립 승인 문서 실제 작성·최종 manifest는
남아 있다. DEV NOT_RUN, 서비스 기동 0, runtime pin 미발급.

## 합성 audit 연결

`approval_gate.inspect(...)`는 환경 인증 reader로 후보/실제 artifact 확인 →
네이티브 검토 2회 → 독립 문서 2회 → 후보/실제 artifact 재확인 → 독립 문서
2회 → 네이티브 검토 2회를 수행한다. 독립 문서는 기존 subject에
`native_review`(업무·최종 판정 ID·CTO/Security ID 순서)를 추가한
`bound_subject`를 승인해야 한다. 이전 판정 또는 판정 ID 없는 문서는
같은 manifest라도 거절한다. 독립 작성자가 최종 네이티브 검토 뒤 문서를
작성해야 하며 SRE는 대신 게시하지 않는다.

결과는 시점 한정 audit이며 실행 허가가 아니다. `approval_verified=false`,
`reusable_permit=false`를 유지한다. API와 파일시스템의 원자 snapshot은
아니며 반환 뒤 철회/파일 변경 가능성이 있다. launcher의 실제 실행할 캡처
bytes 결합과 기동 직전 재조회는 남아 있다. 서비스 executable/launcher와
최종 승인 후보는 미완료이며 runtime pin은 발급하지 않았다.

## 승인 확인과 offline validator 연결

`reviewed_check.check(...)`는 인증된 `approval_gate.inspect` → 기존
`offline_check.check`(입력 캡처·descriptor SHA·private 실행 사본·의미 검증) →
새 인증 audit 순서로 실행한다. 첫 audit가 거절되면 validator를 실행하지
않는다. 검증 중 문서 철회·네이티브 상태 재개·binary 변경·조회 오류가 생기면
성공 결과를 반환하지 않는다. 임시 실행 사본 정리는 offline_check가 맡는다.

반환값은 두 audit 사이의 offline 검사 기록이다. runtime permit이 아니며
`approval_verified=false / reusable_permit=false / services_started=false`다.
실행 인자·home·genesis는 runtime manifest 승인과 별도로 기존 C 의미 검증을
받는다. 프로세스가 종료된 뒤의 파일/승인 변경을 막는 기능은 없다.
이번 연결시험은 합성 승인 reader와 합성 validator subprocess를 사용한다.
실제 Rust validator 재시험 및 서비스 launcher와 기동 직전 gate는 남아 있다.

## 기동 전 검증 CLI

`python3 -B ops/s3-local/reviewed_cli.py`는 `offline_cli.py`와 동일한
필수 옵션에 다음 세 가지 공개 인계 ID를 추가로 요구한다.

- `--native-decision-id`: 이 후보의 최종 CTO→Security 네이티브 판정 UUID
- `--ceo-revision`: `runtime-ceo-approval`의 최신 revision UUID
- `--cto-revision`: `runtime-cto-approval`의 최신 revision UUID

기본 비활성·두 opt-in·절대 경로 요구를 유지한다. ID 누락/중복/축약·
잘못된 UUID·`--serve`·인증 토큰 옵션은 IO 전에 거절한다. 인증은 현재
Paperclip run 환경에서만 읽고 세 인계 ID는 validator 자식 argv에 넣지
않는다. 승인 확인→입력 캡처/검증→새 승인 확인이 전부 성공해야 stdout에
JSON 한 개를 쓴다. 거절은 exit2·stdout0·고정 stderr
`LOCAL_REVIEWED_CHECK_REJECTED`로 표시한다.

이 명령은 offline 감사용이며 서비스 실행 허가를 반환하지 않는다.
`approval_verified=false / reusable_permit=false / services_started=false`를
유지한다. 실제 독립 승인 문서/최종 manifest는 아직 없으며, 합성 reader와
script validator를 이용한 CLI 전체 경로 시험은 실제 승인이나 Rust/C
재검증을 의미하지 않는다. 서비스 launcher와 기동 직전 gate는 후속 작업이다.

## Worker 실행 바이트 준비 (2026-10-07)

`staged_worker.stage`는 인증된 승인 audit → input-set 캡처 → 캡처된 SRE
`bin/s3-local-worker` SHA 대조 → private root0700/file0500 사본 → 새 audit를
연결한다. context 안에서만 사본 경로와 immutable capture bytes를 제공한다.
정상 반환·예외·KeyboardInterrupt에서 임시 사본을 제거하며 원본/home/key는
삭제하지 않는다. 합성 bytes와 reader 시험이며 서비스 실행은 없다.

이 API는 실행 승인 permit이나 완성된 launcher가 아니다. 승인 조회 뒤 철회와
동일 uid 변경을 영구 방지하지 않는다. 실제 managed launcher는 의미 검증,
시작 직전 승인 재조회, 유한 입력/자원 상한과 signal/reap을 연결해야 한다.
서비스 executable·웹 ChainPort·fee0/25 초기화·fault/정리·최종 manifest와
CEO/CTO 독립 승인·CTO→Security 제출은 남아 있다. runtime pin 미발급,
DEV NOT_RUN, 원 G00/ACK 및 부모 blocker 유지.
