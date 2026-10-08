# 관리 웹 실행 경계

`managed_web.run`은 기존 `reviewed_web.prepare`(같은 capture/C 의미 검증/asset 고정)를 호출한 뒤 인증 승인 원문을 다시 대조하고 중단 여부를 확인한다. 그 뒤에만 IPv4 `127.0.0.1:5173` socket을 생성·bind·listen한다. 포트 재사용 옵션은 켜지 않는다. 단일 연결·요청 수 1–10000·수명 1–3600초의 기존 listener 및 제한된 loopback upstream을 사용한다.

생성 뒤 실패/중단은 close한다. listener에 소유권을 넘긴 뒤에는 기존 listener가 close를 책임진다. 결과에는 capture/validator SHA가 포함되지만 `approval_verified=false`, `reusable_permit=false`다. 실행 도중 승인의 지속 유효성을 보장하는 lease가 아니다.

이번 검증: 신규 순수시험4 PASS. 승인 거절/변경·의미 오류/stop에서 socket 생성0, 정확한 audit/prepare/audit/bind/listen 순서, bind/listen 오류·interrupt 정리. 웹 관련 discover 실행37 PASS/0 FAIL은 imported fixture의 중복 수집을 포함하므로 독립 시험수로 합산하지 않는다. 가짜 socket·reader·validator이며 실제 bind/listen/connect0, Rust/C 전체 연결 재시험0.

내부 API이며 관리 CLI/등록 packet 및 인증 private reader/signal scope의 최종 연결이 남아 있다. 실제 서비스 기동은 승인 runtime pin 이후 L-T에서만 수행한다. 새 fee0/25 home/genesis, chain/fault/정리, 최종 build/manifest·독립 승인·CTO→Security 심사가 미완료다.
