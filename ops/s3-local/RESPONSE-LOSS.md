# 직접 방송 응답 유실 주입 경계

L-R 준비 산출물. 실제 서비스 실행은 L-T의 승인 runtime에서만 수행한다.

`response_loss.ResponseLoss`는 기존 `WebProxy.respond(..., exchange)`의 제한된
exchange를 감싸는 내부 시험 API다. 기본 비활성이고 `enable_local_demo=True`와
`allow_unproven_host_space=True`를 모두 요구한다. 이 옵션은 조직 승인이 아니다.
최종 manifest·독립 승인·관리 runtime 경로를 대체하지 않는다.

L-T harness가 정확한 `('127.0.0.1', worker_port)`와 제출할 JSON 본문의 SHA256을
선택하고 기존 bounded `Upstream`을 전달한다. 주입기는 정확한
`POST /dev-local/v1/chain/broadcast` 한 요청만 허용한다. 첫 호출 진입 때 닫히며
오류·중단·재진입·두 번째 호출에는 추가 upstream 전송을 하지 않는다.
전체 응답 반환 뒤 원문을 버리고 고정 오류를 발생시킨다. 기존 WebProxy는 503과
`durable_ack=false`를 반환한다. 조회는 별도의 정상 exchange로 수행하며 자동 TX
재제출은 하지 않는다. 세션 bearer·TX 원문·응답 원문은 report에 저장하지 않는다.

`upstream_attempted`는 callback 호출 시도, `upstream_returned`는 callback 정상
반환일 뿐 방송 또는 체인 확정을 뜻하지 않는다. 잘못된 인증에 대한 응답도
버릴 수 있다. 체인 block/TX/receipt와 자산 보존은 L-T가 별도로 입증해야 한다.
F06/F07의 특정 소켓 경계나 F08의 체인 commit 시점을 이 API로 입증하지 않는다.
현재 DEV는 NOT_RUN이다. 관리 웹 CLI 연결은 아래와 같다.
worker 정산 RPC 및 저장 경계 crash 주입도 별도 남아 있다.

검증: `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest test_response_loss test_web_proxy test_web_upstream -v`
신규4+회귀9=13 PASS/0 FAIL. 메모리 callback/socket 시험이며 실제 네트워크0.
선택된 본문/route/destination·기본 거절·응답 유실→503·callback1회·오류/중단/재진입
뒤 재호출0·보고서 사본/비밀 비노출을 확인했다. Rust/Go/C 재시험0.


## 관리 웹 실행 연결 (준비)

`web_cli.py serve-web-reviewed`의 기존 exact 인자에
`--drop-broadcast-response-sha256 <제출 JSON 원문 SHA256>`을 명시하면
기존 두 opt-in·private approval reader·의미 검증·시작 직전 audit를 그대로 거친다.
옵션 생략 시 기존 정상 Upstream을 사용한다. SHA는 소문자 hex 64자이며
중복/축약/equals 표기/잘못된 값은 reader 및 run 이전에 거절한다.
이 옵션은 worker/validator argv로 전달하지 않는다.

`BroadcastResponseLoss`는 WebProxy가 이미 검사한 요청만 받는 내부 경계다.
로그인·Account·결과 조회 등 기존 허용 route는 정상 Upstream으로 한 번 전달한다.
방송은 지정 본문 해시와 일치하는 첫 요청에서만 Upstream을 한 번 호출하고
응답을 폐기한다. 다른 본문을 먼저 제출해도 방송 lane은 닫히며 이후 자동/수동
재전송을 허용하지 않는다. 결과 조회는 계속 가능하다. 이 세션에서 두 번째
직접 TX를 시험하려면 별도 승인된 실행 세션을 준비한다.

정상 종료 시 managed_web.run 결과의 response_loss 필드로 주입 상태를 반환한다.
CLI는 기존대로 조용히 종료하므로 아직 보고서 영속 게시/관리 packet 생성의 fault
선택 연결은 남아 있다. 기존 packet을 임의 수정해 등록 검사를 우회하지 않는다.
실제 관리 command 등록과 서비스 시작은 수행하지 않았다.

이번 검증: `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest test_response_loss_wiring test_response_loss test_managed_web test_web_cli -v`
신규4+회귀12=16 PASS/0 FAIL. 합성 proxy/upstream/lifecycle/CLI 경계이며 실제
네트워크·B/C·브라우저·체인 시험이 아니다. 로그인→응답 유실→조회→추가 방송 거절,
기본 비활성·잘못된 설정의 socket0·exact CLI 배선과 입력 거절을 확인했다.
