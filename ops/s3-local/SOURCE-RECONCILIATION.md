# C/L-D 병합 결과와 고정 commit 대조

compare_candidate는 외부 보고서를 신뢰하지 않고 exact 부모/공통 조상에서 병합 출처를 다시 계산한다. 후보 commit의 bytes/mode·누락·추가·미해소 충돌을 비교하며 canonical reconciliation 원문 SHA256과 후보 head/tree를 기록한다. 일방 삭제와 미해소 충돌을 구분한다. manifest 승인 조건/봉인 예외는 변경하지 않았다.

- 신규4+병합 회귀3 = 7 PASS/0 FAIL. content/mode/누락/추가·충돌·심볼릭 링크·약식 ref 거절, exact commit과 dirty worktree의 분리, 일방 삭제를 확인했다.
- 실제 candidate: `eb935f7412c8ace93535b9663ceac4ffaaebd0fd`, tree `cfc6423eef98a51354d3f7a6d73d1ef550dccbed`.
- C20c0cd9/L-D46546d3의 exchange/ 373파일 병합 재현 결과와 정확히 일치. missing0/changed0/added0/unresolved0. reconciliation SHA256 `14e24f75b64ec17e24e294e0c01ad14f9f2bc589ab99918141d559cdf883d678`.
- 이는 종전 단일 C/L-D head별 대조 차이를 설명하는 재현 근거다. 이전 실패 기록을 삭제하거나 승인으로 바꾸지 않는다. working_tree_checked/approval_verified/sealing_exception_granted/runtime_approved는 모두 false.
- 신규 시험은 임시 Git 저장소, 실제 대조는 기존 Git 객체 읽기만 사용했다. 최초 PYTHONPATH 누락으로 unittest import 실패 후 경로 보정하여 7개 통과. 제품 Rust/Go build/test·서비스/START/RPC0.

재현: `PYTHONPATH=ops/s3-local python3 -m unittest test_reconciled_candidate.Candidate test_source_reconciliation.Reconciliation -v`. 실제 대조는 source_reconciliation.compare_candidate(root, exact_C, exact_LD, "exchange/", exact_candidate, scratch=run_scratch)를 호출했다.

남은 SRE 범위: exact 병합의 독립 검토 출처 결합·명령별 fault 공백·전체 통합 후보 commit/build/다섯 descriptor manifest·CEO/CTO 독립 승인·CTO→Security. 이번 결과만으로 최종 후보를 봉인하지 않는다. runtime pin 미발급·DEV NOT_RUN·€0·G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 유지.

정리: 임시 Git 저장소는 시험 종료 후 제거. 전용 binary/build cache 생성0. API snapshot은 첨부/work product 게시 확인 후 전후 bytes 기록과 함께 제거. 활성 cache·소스/home/키/원장/증거는 보존한다.
