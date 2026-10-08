# Private helper 호출 결합 — NUS-73

기준 eb935f7 위 누적 SRE 미커밋 변경. Helper::verify가 경로를 외부로 반환하지 않고 private helper를 직접 bounded child에 전달한다. 호출 전후 SHA/권한/inode를 검사하고 stop을 재확인한다. 성공 응답 중 파일이 바뀌어도 거절한다. 같은 uid의 악의적 변경을 완전히 격리하는 sandbox는 아니다.

worker 컴파일 PASS. 신규2+기존 helper3/child3 = 8 PASS/0 FAIL. 합성 subprocess이며 실제 Go 서명 검증/실제 Rust-C startup은 이번 재시험하지 않았다. 서비스/START/RPC/방송0. API는 입력의 인증 owner·freshness를 보증하지 않는다. 인증 방송 HTTP 호출점·결과/browser·새 home/genesis·chain/web/fault/정리·최종 manifest·독립 승인/CTO→Security는 남아 있다. 다음 소유자 SRE. runtime pin 미발급·DEV NOT_RUN·€0. G00/ACK와 부모 blocker 유지.
