# 인증 직접 방송 adapter — NUS-73

기존 Rest 세션을 같은 header/peer/origin으로 조회하고 인증 owner의 trusted same-H Account만 helper 입력에 사용한다. canonical base64 TxRaw·owner·등록키·genesis·chain ID·account number·sequence를 기존 Go 검증 경계에 전달한다. helper 성공 뒤 원 Account 수신 시각 기준 2초와 원 Observation/세션/gate를 재확인한다. 실패·철회·취소 시 방송하지 않는다. 방송은 한 번이고 접수/오류 모두 SUBMISSION_UNKNOWN이며 재시도하지 않는다.

신규 순수시험3 + 기존 방송 회귀3 = 6 PASS/0 FAIL. 인증/query/helper/send는 주입 callback이며 실제 Go 검증/로그인/서비스/RPC 시험이 아니다. public wrapper의 기존 Rest/ChainRead/Helper API 타입 연결 컴파일 PASS. 첫 compile argv 인덱스 오류를 수정하고 실패 로그를 보존했다. 나머지71 모듈시험 미실행.

worker router/lifecycle로 전달하는 연결은 다음 SRE 단계다. 결과 HTTP·browser ChainPort·새 home/genesis·chain/web/fault/정리·최종 manifest·독립 승인/CTO→Security가 남아 있다. 서비스/START/RPC/방송0·runtime pin 미발급·DEV NOT_RUN·€0. G00/ACK·부모 blocker 유지.
