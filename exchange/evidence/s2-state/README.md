# S2 EngineState 직렬화 체크포인트

기존 시퀀서에 계약 EngineState 투영과 STATE domain hash를 추가했다. 원 admission_seq 순 주문·원문/서명, raw owner/숫자 epoch/id 순 binding, 실행 순 outbox를 유지한다. C/R/D/P/A는 정수 문자열이다. fill에 최초 snapshot·maker/taker·fee·최악 D·net P와 HELD_S2/submission_enabled=false를 보존한다. epoch 정정은 lifetime filled를 감소시키지 않고 corrected 누계·reason·revision만 바꾼다. 주문 revision은 한 명령의 변경마다 한 번 증가한다.

## 검증

- `cargo test --offline --locked --manifest-path exchange/Cargo.toml --test s2_sequencer`: 19 PASS (신규 3), 시험 표시 0.22초.
- `cargo clippy --offline --locked --manifest-path exchange/Cargo.toml --all-targets -- -D warnings`: PASS.
- `S2_STATE_PROJECTIONS=../exchange/evidence/s2-state/projections.json`을 위 시험에 지정해 출력. Cargo integration test의 cwd는 exchange다.
- `python3 exchange/tools/check_s2_projection.py exchange/evidence/s2-state/projections.json`: 초기/매도/부분 체결/취소/epoch 정정 5개 실제 Rust 출력이 고정 계약 fixture validator 통과. 일반 jsonschema 패키지는 설치되지 않았으며 새 의존성을 추가하지 않았다.
- 중복/동일 snapshot hash 불변, mode별 hash 분리, 정정 후 원본 snapshot/서명/binding 유지, 결정적 거절을 포함한 전체 hash 재계산 일치.
- 테스트 데이터의 두 owner/키/잔고/체인 snapshot은 합성이다. 처리량·CPU·RSS 미측정이며 0.22초는 개발 소요 추정이 아니다.

## 남은 범위

후보 상태 투영이며 영속 저장·LOCAL_ACCEPTED를 제공하지 않는다. mode는 후속 서비스 게이트가 공급해야 한다. EngineState만 deserialize하여 복원하면 안 된다. 원 결과/receipt 인덱스, 취소 서명, 출금 동결 관측 높이와 로컬 요청 ID binding은 WAL 입력 재생으로 복원할 대상이다. 출금 prepare/abort ID binding, 서비스 전역 gate, CommandResult/JournalRecord/receipt 연결, 최대 정정 직렬화 크기 확보, 디스크 재생/crash·서비스 통합은 미완료다. 제품 PASS·CTO/Security 검토 요청이 아니다. 공통 protocol은 수정하지 않았다.
