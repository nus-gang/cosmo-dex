# 관리 Chain foreground 명령

L-T에서 exact runtime 승인 후 Paperclip manual runtime command로만 사용한다. L-R에서는 등록/서비스 시작을 수행하지 않는다.

`python3 -B chain_cli.py serve-chain-reviewed`에 다음 옵션을 모두 지정한다:

- 절대 경로: `--bundle`, `--artifacts`, `--input-set`, `--effective-profile`, `--home`, `--scratch`, `--approval-socket`, `--pid-mailbox`
- `--runtime-pin` 실제 manifest SHA256, `--local-demo-profile s3-dev-local/1`, `--acknowledge-unproven-space`
- `--native-decision-id`, `--ceo-revision`, `--cto-revision`: 같은 후보의 실제 승인 UUID
- `--rpc 127.0.0.1:26657`, `--p2p 127.0.0.1:26656`, `--peers ID@127.0.0.1:PORT,ID@127.0.0.1:PORT,ID@127.0.0.1:PORT`: 새 검증인 세 peer. ID는 40자리 소문자 hex, 주소/ID 중복 금지. 포트 1024–65535.
- `--lifetime-seconds`: 1–300 정규 십진 정수

private broker는 현재 run 인증 reader를 제공한다. 사전 생성한 nonce mailbox에 launcher/child PID를 기록한다. 상속 mailbox schema의 `worker_pid`는 여기서는 Chain child PID다. 자동 삭제/재사용하지 않는다. 전체 descendant inventory는 별도 검증 대상이다.

CLI는 signal scope→private reader→descriptor/private 사본→B preflight→READY→시작 직전 새 audit→START→foreground 감독을 조합한다. 오류 진단은 고정 문자열만 출력한다. 성공 exit0도 전체 cleanup 또는 DEV PASS 의미가 아니다. 관리 등록 packet/session의 Chain 전용 endpoint/종료 inventory 연결은 후속 작업이다.

이번 검증: CLI 4 PASS, 실행 supervisor/reader/stage mock. 열린 stdin 거절은 실제 단명 Python subprocess. 실제 Chain/Go/Rust 재시험 및 서비스 시작0.
