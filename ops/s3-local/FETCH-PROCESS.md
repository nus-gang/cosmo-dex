# 초기 Snapshot fetch 부모 IO 감독

`fetch_process.fetch_captured`는 descriptor/private 사본과 인증을 마친 caller가
쓰는 내부 IO 함수다. 고정 fetch-captured argv와 capture SHA, loopback literal,
정규 pin, 명시적 opt-in, 절대 profile 경로를 받는다. 원문을 유한 stdin으로
보내며 환경 자격증명/loader 설정을 전달하지 않는다. 기본 10초(최대60초),
stdout 16MiB, stderr 4096 bytes, 50ms stop 점검 및 process group kill/reap을 적용한다.
수신 완료·exit0·stderr0·비어 있지 않은 응답만 원 bytes로 반환한다.

FetchFailure.partial_raw는 실패 때 상한 내 수신한 비신뢰 bytes이며 오류 메시지에는
포함하지 않는다. Snapshot 의미/완전성·최종 승인·home 생성 허가가 아니다.
KeyboardInterrupt 및 OS 예외는 전파하고 child를 정리한다. 이 경우 원문 반환을
보장하지 않는다. caller의 증거 게시·재승인/create 연결은 아직 남아 있다.

검증: 합성 subprocess 신규5 PASS. exact argv/SHA/원문·환경 격리, 입력 거절 spawn0,
nonzero/stderr/빈 응답/출력 초과, timeout/stop/interrupt 및 종료하지 않는 child와
pipe를 유지하는 descendant의 수명 제한. 최초 fixture0500 덮어쓰기 오류는
매 case 기존 fixture unlink로 수정했고 실패 로그를 보존했다.
실제 Rust fetch/인증 reader/descriptor 연결과 실제 RPC는 이번 시험에 포함하지 않는다.
서비스/listener/RPC0, runtime pin 미발급, DEV NOT_RUN.
