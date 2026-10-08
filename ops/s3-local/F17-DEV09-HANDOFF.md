# F17/DEV09 승인 해석 인수

2026-10-07 · SRE · 문서 적용만 수행. 제품·계약·manifest 변경 및 실행 승인 없음.

[NUS-54](/NUS/issues/NUS-54)의 done, Security→QA completed/approved와 최종 판정 `1d632638-3b18-4270-ae4f-392b0dc86c98`를 인증 API에서 확인했다. CTO 원문 SHA256은 `a773c4cf1464263e21cc9f389c3922972c02bd37186ec90d444058e0894f35e7`이다.

| 승인 출처 | exact revision |
|---|---|
| [CTO 적용 판단](/NUS/issues/NUS-54#document-local-demo-f17-interpretation) | `1ec0db2d-2060-4b41-b57f-7896cd0d5d3d` |
| [Security](/NUS/issues/NUS-54#document-local-demo-f17-security-review) | `4399f523-a0d1-4f58-a68f-0934a0c9340a` |
| [QA](/NUS/issues/NUS-54#document-local-demo-f17-qa-review) | `9384d85c-99f5-4e79-9d8c-5839a9e56586` |

대상 C head `3802bf0cd3dabd36ee495a60ac38cfa9a9cec9c7`, tree `c11a65866568e7da045baca350b5c67804bafec1`. A 규범 head와 승인 manifest는 변경하지 않는다. 이 해석을 component source 예외 또는 runtime pin으로 쓰지 않는다.

## 실행자가 보존할 판정

원 F17은 `after snapshot temp fsync before rename` / `OLD_VALID_SNAPSHOT_PLUS_WAL` / **NOT_RUN** 그대로다. marker 파일과 메모리 Arc는 파일 snapshot checkpoint가 아니다. 현재 개발 경로에 F17 이름을 맞추는 새 checkpoint는 요구하지 않는다. 표준 S2/S3 snapshot 요구의 전역 면제도 아니다.

| DEV09 경계 | 이후 검증할 결과; 이번 실행 결과 모두 NOT_RUN |
|---|---|
| after_marker_sync | temp/transaction·원문 보존, 같은 home 두 번 open 거절; 거절을 성공 replay로 합산하지 않음 |
| after_marker_rename / marker_dir_sync / after_marker_dir_sync | transaction 제거 전 파일 집합·digest와 닫힘 확인; 종료와 errno 반환 구분 |
| after_commit / before_publish | complete prefix 두 번 의미 재생, seq/hash·결과·C/R/D/P·book/FIFO/cursor/correction revision 일치·중간 공개0 |
| before_response | 원 signed request 결과·독립 개발 receipt ledger 대조, 추가 지급/정정/방송0 |

frame/object/marker/방송/receipt/publish **각 종료 경계별 3회·2회 replay/open**와 실제 명령/도달 phase/원문 digest/expected diff를 기록한다. F12/F16은 해당 경제 명령·정정 조건을 별도로 입증한다. DEV10 errno/부분 write/cap은 별도 시험이며 host ENOSPC·전원 손실·표준 ACK 증거로 합산하지 않는다. 자동 삭제/truncate/재봉인으로 실패를 복구 성공으로 바꾸지 않는다.

## 남은 L-R 작업

[마감표](FAULT-READINESS.md)의 attempt/방송/receipt 전용 장벽, 종료 목록과 실행 안내, 교차 component 병합 심사 출처, 최종 candidate/build/다섯 descriptor manifest, CEO/CTO 독립 승인 및 CTO→Security는 여전히 남아 있다. 실제 서비스 실행·DEV09 인수는 승인 pin 이후 [L-T](/NUS/issues/NUS-74)다. 이 문서는 어느 행에도 PASS를 부여하지 않는다.

규범 해석 변경은 CTO→Security→QA, 저장 구현 변경은 원 [C 개발](/NUS/issues/NUS-70)→CTO→Security로 반환한다. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 및 부모 blocker 유지.
