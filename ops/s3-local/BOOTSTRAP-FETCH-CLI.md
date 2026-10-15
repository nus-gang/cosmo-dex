# 초기 Snapshot 조회 명령

`bootstrap_fetch_cli.py`는 현재 run의 인증 reader를 사용해 기존 fetch/checker 경로를 호출한다. 실제 RPC 조회는 승인 pin 이후 L-T에서만 실행한다. 이번 L-R 검증은 mock fetch와 filesystem/거절 시험이다.

기존 `bootstrap_cli.py`의 필수 인자 전부에 `--chain-rpc 127.0.0.1:26657 --evidence-root /absolute/private/evidence`를 추가한다. 두 opt-in·정확한 승인 decision/revision이 필수다. evidence-root는 기존 canonical root0700이며 `snapshot-fetch.raw`가 없어야 한다. timeout은 기존 10초 상한을 사용한다.

조회 전 파일0600을 no-replace로 예약하고 file/root fsync를 완료한다. 성공 원문과 FetchFailure의 비신뢰 부분 원문을 같은 예약 파일에 보존한다. 실패·중단·fsync 오류 후 파일을 지우거나 재시도하지 않는다. partial bytes가 있더라도 Snapshot/승인/초기화 허가가 아니다. 성공 stdout은 hash/크기와 snapshot_verified=false·home_created=false·reusable_permit=false만 보고한다. 실패 stdout은 비어 있고 고정 오류와 exit2를 낸다.

조회 원문은 후속 C 의미 검증과 별도 create 승인이 필요하다. create CLI·chain 관리 launcher/fault·최종 manifest·독립 승인/CTO→Security는 아직 남아 있다. 서비스 기동·runtime pin·DEV PASS를 발급하지 않는다.
