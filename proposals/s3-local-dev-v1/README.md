# 로컬 개발 계약 심사 묶음

활성화되지 않은 ALLOC-67-06 후보다. 현재 실행 명령은 아래 read-only 후보 검사뿐이다. runtime/REST/worker 시연 명령은 구현 후 기존 담당이 고정한다.

```sh
python3 -B proposals/s3-local-dev-v1/verify.py
```

검사기는 상속 rc3204파일과 후보 hash, fee0/25의 정확한3개 override, cap·기본 비활성·원 gate 및 NOT_RUN 표기를 확인한다. 실제 실행 안전성·native 보안/QA·FS 지원 검증을 대신하지 않는다. 재현용 기존 Rust/Chain/arena 명령과 이번 결과는 NUS-67 local-development-path 문서와 첨부에 있다.

CONTRACT.md와 effective profiles는 정확한 재심사 대상이다. 후보 manifest는 자기 hash를 제외하고, handoff manifest가 commit/tree와 묶는다. 승인 전 구현/활성화·표준 ACK 변경0. 원문·기존 성공/실패는 보존한다.
