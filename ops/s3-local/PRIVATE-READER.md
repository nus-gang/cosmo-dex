# 현재 run 승인 조회용 private IPC

2026-10-07 · NUS-73 L-R 진행. 서비스 기동·runtime 승인·DEV 검증이 아니다.

`private_reader.broker(scratch)`는 현재 run의 `Reader.from_environment()`를
메모리에 유지한다. credential을 파일/command/config/자식 환경/IPC 응답에
전달하지 않는다. runtime 쪽 `PrivateReader(endpoint)`는 정확히 현재 업무와
CEO/CTO 승인 문서의 GET 세 경로만 요청한다. 응답은 매번 새 조회이며 캐시나
재사용 permit이 아니다. 임의 URL·다른 업무·쓰기 경로는 없다.

새 root0700/socket0600, 소유 uid, 링크 거절, connect 전후 inode/권한 대조를
사용한다. 같은 uid의 악성 프로세스는 격리하지 못한다. 경로/소유자 검증이
Paperclip 관리 runtime 소유권 인증을 대신하지 않는다. IPC endpoint는 신뢰된
현재 run이 향후 전용 runtime 설정에 연결해야 하며 manifest에서 가져오지 않는다.

요청은 길이 prefix+EOF, 최대512bytes, 단일 ASCII 경로다. 응답은 길이
prefix+EOF와 dict JSON, 최대32MiB다. 요청2초·응답 전송2초, client 전체
교환12초, 최대128연결·lease300초다. 잘못된 연결도 횟수를 소비한다.
upstream은 기존 Reader의 응답/시간 상한을 사용한다. 실패 시 본문/예외를
전달하지 않고 연결을 닫는다. context 종료는 thread 합류 뒤 socket/root를
제거한다. run 종료 후 인증 재사용이나 자동 broker 재시작을 제공하지 않는다.
Mac AF_UNIX 경로 길이 제한에 걸리면 bind 실패로 정리하며 다른 위치로
자동 fallback하지 않는다. scratch는 Paperclip run scratch를 사용한다.

검증: 합성 upstream + 실제 private Unix IPC 5 PASS/0 FAIL. fresh 응답,
경로/권한/링크·truncation/oversize/trailing bytes, upstream 오류 비노출,
요청 횟수·만료 deadline, interrupt/bind 실패 정리. 첫 sandbox에서는 bind
Operation not permitted(3 ERROR, wire 시험1 PASS); 승인된 확대 실행으로
4 PASS 후 응답 크기/정리 보강 최종5 PASS. 이 수치는 중복 합산하지 않는다.
실제 API reader 조합·관리 CLI/설정 연결·서비스 시작은 이번에 수행하지 않았다.

다음 SRE 작업: broker를 현재 L-T run 수명과 관리 CLI에 명시적으로 연결하고
검증한다. 이어 웹 ChainPort, 새 fee0/25 초기화, chain/web/fault/정리, 최종
manifest·독립 승인·CTO→Security 검토를 완료해야 한다. 원 G00/ACK와 부모
blocker 유지, runtime pin 미발급, 실제 서비스/Chain RPC0, 비용 €0.

## 관리 CLI 연결 (2026-10-07 후속)

`managed_cli.py serve-reviewed`와 `runtime_config.worker_config`는 이제
`--approval-socket /absolute/private/root/s`를 필수로 받는다. 이 값은 worker
argv나 input-set으로 전달되지 않는다. CLI의 실행 범위 안에서 ContextVar로
private reader를 선택하여 staging/의미 검증/READY/START 직전의 모든 audit가
같은 broker를 통해 새 GET을 수행한다. IPC 실패 뒤 환경변수 Reader fallback은
없으며 오류/interrupt 뒤 원래 context를 복원한다. 일반 offline audit 경로의
현재 run Reader 사용은 유지한다.

L-T는 현재 run의 `broker(PAPERCLIP_RUN_SCRATCH_DIR)` context 안에서 반환된
endpoint를 전용 관리 runtime 명령에 넣고, foreground runtime 종료까지 broker를
유지해야 한다. broker 종료/lease 만료 후 그 명령은 승인 조회에 실패한다.
토큰을 runtime 설정/명령에 넣지 않는다. 이 문서의 연결 규칙만으로 runtime을
등록하거나 시작하지 않는다. 실제 L-T 관리 API orchestration은 아직 남았다.

이번 확인: CLI/설정/범위 복원 9 PASS, 합성 승인 + 실제 IPC audit/철회 시험을
별도 기록했다. 실제 인증 API → broker → native review는 현재 in_progress를
NATIVE_REVIEW_REQUIRED로 거절했고 socket/root 정리를 확인했다. Rust worker와
서비스 시작은 이번 재시험하지 않았다.
