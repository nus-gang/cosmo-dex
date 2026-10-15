# Chain 종료 writer 관측

`chain_session.session`은 관리 stop 확인 뒤 mailbox PID → RPC/P2P port → 기존 `writer.dev.lock` 순서로 관측한다. `chain_writer_release.check(home)`은 Chain Go executable과 같은 비차단 배타 flock을 한 번 획득하고 반환 전에 놓는다. lock 생성/삭제/truncate·재시도는 하지 않는다. root0700/file0600·현재 UID·단일 regular inode·canonical 경로를 요구하며 획득 전후 inode/metadata/경로를 비교한다.

성공은 그 시점의 `writer_lock_reacquired=true`이다. 지속 exclusion·전체 descendant 종료·전체 cleanup·물리 내구성 증명이 아니다. `cleanup_complete_verified=false` 유지. 실패/interrupt 뒤 fd를 닫고 session은 앞선 process/port 근거와 mailbox를 보존한다.

검증: `PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=ops/s3-local python3 -m unittest test_chain_writer_release test_chain_session test_authenticated_chain_session -v` — 신규5+회귀9=14 PASS/0 FAIL. 실제 임시 filesystem/flock·합성 control/probe. 기존 Go executable의 flock 구현을 대조했으나 Go 프로세스와의 교차 lock 시험은 이번 미실행. 서비스/START/RPC0, DEV NOT_RUN.
