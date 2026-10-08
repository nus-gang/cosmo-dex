# 종료 증거 — 구현 중

L-R에서는 순수 주입 시험만 실행했다. 실제 서비스/관리 start·stop/Chain RPC는 0회다.

- `runtime_evidence.stopped`: 지정 command/workspace의 성공 stop operation과 fresh GET을 대조한다. 호스트 종료 증거는 아니다.
- `port_release.check`: 고정 설정의 loopback TCP endpoint를 동시에 bind한 시점의 가용성만 확인한다. listen/connect하지 않고 즉시 close하며 예약을 유지하지 않는다.
- `process_release.check`: launcher가 실제 실행에서 기록한 명시적 PID 목록(2..2^31−1, 최대32개, 중복 없음)을 signal 0으로 한 번씩 조회한다. ESRCH만 listed_pids_absent=true다. EPERM/EACCES/기타 오류·살아 있는 PID·zombie·재사용 PID는 거절한다. kill/종료 재시도는 하지 않는다.

PID 목록은 API의 stopped 문자열이나 임의 사용자 입력에서 추정하면 안 된다. 향후 launcher가 시작 때 보존한 부모/자식 목록과 결합해야 한다. 이 함수 자체는 목록 완전성·프로세스 트리 종료·writer lock 해제·포트 해제를 입증하지 않는다. PID 부재는 조회 시점 한정 관측이며 이후 PID 재사용을 방지하지 않는다. 손상 home이나 예상 밖 증거는 자동 삭제하지 않는다.

다음 연결: 관리 session의 실제 PID inventory 수집과 검증, writer probe의 관리 session 연결, 종료 오류 보존. 웹 ChainPort·초기화/chain/web/fault/최종 manifest·독립 승인은 별도 미완료다. runtime pin 미발급, DEV NOT_RUN.

검증: `cd ops/s3-local && python3 -m unittest -v test_process_release` — 신규4 PASS/0 FAIL. os.kill·clock 모두 주입 또는 mock이며 실제 signal0 호출0. 다른 기존 시험은 재실행하지 않았다.


`writer_release.check`는 시작 전 검증한 동일 capture bytes와 worker 인자를 받아
SRE descriptor에 결합된 offline validator를 private 실행 사본으로 실행한다.
C `Engine::open`과 복구/signer 준비 후 drop·child reap이 성공해야
`writer_reopen_verified=true`를 반환한다. lock 점유/손상/바이트 변경/IO 오류는
고정 오류로 거절하며 재시도·lock 파일 삭제·home 생성/수리·서비스 시작은 없다.
관측 사이 재점유를 막거나 commit 불변을 자체 증명하지 않으므로 해당 flag는
false다. 이 내부 API의 호출자는 원 capture와 해당 worker 인자의 결합을 유지해야 한다.
관리 stop→PID/port/writer 조합의 최종 연결과 실제 서비스 종료 시험은 미완료다.

이번 writer 검증: 순수4+합성 descriptor subprocess1 PASS; 실제 Rust validator/C
fee0/25 연결1 PASS(22.45초), 각 home 두 번 replay/commit 불변을 외부 Rust
fixture에서 확인했다. 실제 worker/service 시작0. 기존184 Rust 시험 미실행.

## 종료 목록 결합 (이번 변경)

`release_inventory.probes`는 trusted launcher가 수집한 PID/endpoint 목록,
동일 capture bytes·artifact 목록·worker argv와 validator SHA를 복사해 고정한다.
반환된 세 probe를 `_release_session(..., **probes)`에 전달한다. 결과의 PID,
endpoint, validator SHA가 고정 입력과 다르면 거절한다. 각 probe는 오류나
interrupt를 포함해 한 번만 호출할 수 있다. 입력 객체의 후속 변경은 반영되지 않는다.

신규4 + session/control-plane 회귀8 = 합성12 PASS/0 FAIL.
실제 process/port/writer 검사·관리 API start/stop·서비스 실행0.
실제 launcher PID 수집과 pinned command에서 endpoint 추출은 아직 남아 있다.
목록의 완전성·전체 cleanup·조직 승인·DEV PASS를 주장하지 않는다.

### 2026-10-07 inventory→writer 실제 입력 경계 수정

`release_inventory`의 `artifacts` 입력은 validator가 있는 절대 디렉터리 경로다.
이전 dict-only 검사는 mock 경로만 허용하여 실제 `validate_snapshot` 호출을
막았다. 이제 `str`/`Path` 절대 경로를 고정하고 상대 경로·`..`·dict는 거절한다.
경로 고정은 파일 bytes 고정을 뜻하지 않는다. writer 점검은 원 capture의 SRE
 descriptor SHA와 실제 validator를 재대조한 뒤 private 사본으로 실행한다.

`test_inventory_writer_transport.InventoryWriterTransportTest`는 실제 파일과
합성 script subprocess를 사용한다. 한 번 실행·변조 거절·사본 정리 및 fee0/25
PreparedSession→mailbox→inventory→writer 연결을 검증한다. 제어 API와
process/port 관측은 mock이며 실제 C store/서비스/포트 종료 PASS가 아니다.
전체 descendant 목록과 cleanup 인증은 계속 false다.
