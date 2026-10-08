# ChainPort 확정 결과 bridge 진행

[NUS-73](/NUS/issues/NUS-73) SRE. base eb935f7 위 누적 SRE 미커밋 소스.

`ChainRead::direct_result(anchor, hash)`를 추가했다. 인증/승인된 launcher가 공급하는 trusted Snapshot과 소문자 SHA256을 받는다. /tx는 위치 힌트일 뿐이다. exact H Snapshot을 기존 승인 L-D decoder로 읽고 block/block_results의 원문·H/hash/chain/count/index/TX를 기존 C proof로 검사한다. 결과는 L-E ChainPort.result의 context/tx_bytes/tx_hash/height/code/state에 맞춘다. /tx tx_result는 사용하지 않고 검증한 block_results code만 사용한다. nonzero는 사용자 TX의 REJECTED_FINAL이며 batch VOID/정정 또는 자산 변경 근거가 아니다.

원 tx hint·snapshot·block·results·TxRaw를 Objects에 보존한다. 없는/미래/0 높이, hash/bytes/index 불일치, 중복 포함, 잘못된 코드/원문, IO를 거절한다. 자동 재시도·방송·서명·엔진 변경은 없다. 공개 API나 브라우저 route는 아직 연결하지 않았다. trusted local RPC 모델이며 독립 consensus/light-client 검증이 아니다. 호출자는 검증된 최신 anchor를 제공해야 하며 브라우저가 Snapshot을 지정할 수 없다.

## 검증

설치 Rust 1.92.0·기존 dependency rlib로 API/시험 컴파일 PASS. 신규 순수4 + 포함 증거 회귀7 = 11 PASS/0 FAIL. 나머지 모듈39개는 미실행. 합성 TxRaw/모의 fetch이며 실제 네트워크0. 처음 standalone module의 pub(super) 컴파일 오류는 wrapper로 보정했다. 첫 fixture의 RPC 숫자에 journal canonical encoder를 적용해 신규4 실패했고 일반 JSON encoder로 보정한 뒤4 PASS. 실패/통과 로그와 exact argv를 보존한다.

## 다음 작업

SRE: 웹 ChainPort Account/방송 및 인증 HTTP/브라우저 배선, 새 fee0/25 home/genesis·chain/web launcher·fault/정리·최종 5 descriptor manifest·독립 CEO/CTO 원문 승인·CTO→Security 심사. 기존 관리 session CLI 완료분은 유지한다. 실제 서비스 기동은 L-T.

관리 API start/stop·개발 서비스·Chain RPC0, pin 미발급·DEV NOT_RUN·€0. 원 G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED와 부모 blocker 유지.
