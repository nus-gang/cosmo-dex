# 응답 유실 fault 증거

관리 웹 command에서 `--drop-broadcast-response-sha256 <exact-body-sha256>`를 선택하면 `--fault-evidence-root <absolute-private-directory>`도 필수다. 두 옵션은 함께만 허용한다. 등록 packet에도 두 값을 포함하므로 디렉터리 변경은 exact command 변경이다. 기존 fault command는 재생성·등록 대조가 필요하다.

호출자는 새 root0700 디렉터리를 준비한다. 승인/의미검증 뒤 `response-loss.jsonl`을 no-replace/file0600으로 예약하고 file/root fsync를 완료한 뒤에만 PID callback과 socket 생성으로 진행한다. 기존 기록은 덮어쓰지 않는다. 정상 종료, 오류, interrupt의 마지막 상태를 추가하고 fsync한다. 일반 실행에는 증거 파일을 만들지 않는다.

파일에는 body SHA와 관측 flag만 저장하며 bearer/TX 원문/예외 내용은 저장하지 않는다. 응답 폐기는 체인 확정 증명이 아니다. final 누락/부분 기록 또는 fsync 실패는 UNKNOWN이다. SIGKILL/전원 손실 후 자동 재시작/기록 삭제/재시도하지 않고 원문을 게시·보존한다. 실제 서비스와 fault 시연은 승인 pin 이후 L-T에서 수행한다.
