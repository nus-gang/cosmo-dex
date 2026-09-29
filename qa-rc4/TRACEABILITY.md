# rc4 T01~T16 인수 추적표 — 원래 의미 보존

[NUS-9 원문 인수 기준](/NUS/issues/NUS-9#document-acceptance) revision `d4f05eaa-dd46-487b-a520-00a1cd9cda0d`, 작성 Tester·검토 CEO를 인수했다. 원문 설계서 14쪽과 번호/필수 시나리오/통과 조건을 대조했다. M0 문서를 수정하거나 재개하지 않았다. QA 실행·증거 판정 담당은 신규 QA, Security 독립 검토, CTO 통합·CEO 인수다. 기존 주담당의 Tester 표기는 작성 이력으로 보존한다.

측정 양식 revision `8b1991b5-e355-40be-b916-c3047f9e0d86`, M0 plan revision `43c89779-7be5-49f5-aed7-b4ae3de0a8a4`, 현 승인 Plan 3 revision `d9c0676b-9e96-48b1-bf40-4618e1c48c15`. 초기 팀 승인 revision `b6f74341-43e6-49c9-853e-75efc4fa1732`는 인수 이력이고 S0의 승인 범위는 Plan 3이다. 원문 PDF·문서 사본과 해시는 첨부에 보존한다.

|ID|원문 필수 시나리오|원문 통과 조건|원래 주담당/협업|제품 판정|S0 부분 증거와 미실행 경계|
|---|---|---|---|---|---|
|T01|ML-DSA 위조·다른 chain ID/genesis/order domain 재사용|주문과 체인 정산 모두 거절|Security / Chain·Exchange·Wallet|NOT_RUN|rc4 실제 서명/domain·key type 회귀 PASS. 주문 API와 chain settlement 미연결|
|T02|같은 order ID, 다른 본문; 부분 체결 합계가 서명량 초과|바인딩 충돌·초과 부분 거절|Chain / Exchange·Security|NOT_RUN|정수·누적량 함수 일부 PASS; 영속 binding/원자 누계 NOT_RUN|
|T03|주문 취소와 매칭 동시 도착|단일 seq 순서로 재생 후 동일 결과|Exchange / Settlement|NOT_RUN|IOC callback 단위 PASS; 경쟁 seq/WAL replay NOT_RUN|
|T04|출금→정산 및 정산→출금 블록 순서|확정 잔고 음수 없음, 무효 잠정 체결 정정|Chain / Exchange·Settlement|NOT_RUN|규약만 확인; 두 블록 순서/epoch 원자 증가 NOT_RUN|
|T05|두 시장에서 같은 잔고를 동시 예약|글로벌 R+D <= C 유지|Exchange / Tester|NOT_RUN|공통 장부 및 동시 예약 미구현; NOT_RUN|
|T06|한 배치에 정상·불량 체결 혼합|자산 이동 0, 배치 전체 실패|Chain / Security|NOT_RUN|batch fixture/모의 판정만 존재; 원자 상태 이동 NOT_RUN|
|T07|같은 batch/fill 재전송 및 이전 해시 변경|추가 정산 0, 불일치 경보|Settlement / Chain|NOT_RUN|receipt·멱등 모의 adapter PASS; 실제 추가 정산 0 NOT_RUN|
|T08|WAL·ACK·outbox 경계 중단, 활성 AZ 손실 직후 대기 AZ 승격|마지막 ACK 주문·취소까지 재생 결과·fill ID 동일|Exchange / SRE·Settlement|NOT_RUN|WAL/ACK/outbox·AZ 손실 미구현/미실행|
|T09|RPC timeout, chain commit 뒤 응답 소실|체인 seq/hash 조회 후 중복 지급 없음|Settlement / Chain·SRE|NOT_RUN|timeout/과거 receipt 모의 adapter PASS; 실제 chain/RPC NOT_RUN|
|T10|활성 매칭 두 대 분리 운용·구 리더 복귀·늦은 배치|한 대만 쓰기/서명 가능, 구 운영자 epoch 거절|SRE / Exchange·Chain|NOT_RUN|분할 리더·fencing·실제 epoch commit NOT_RUN|
|T11|bank.MsgSend/MultiSend, authz, 모듈 경유 P2P|스테이블 서비스 수수료 무단 우회 불가|Chain / Security|NOT_RUN|실제 bank/authz/module route 미연결; NOT_RUN|
|T12|송금액·서비스 수수료 원자성, feegrant 소진|실패 시 가치 이동 없음, 한도 초과 차단|Chain / Wallet·SRE|NOT_RUN|송금 codec PASS; 실제 자산/feegrant 원자성 NOT_RUN|
|T13|operator·릴레이어 중단 뒤 사용자 직접 출금|체인 확정 잔고만, 독립 접근·가스 경로|SRE / Chain·Wallet·Security|NOT_RUN|브라우저 메모리 복구만 PASS; 독립 RPC/가스 직접 회수 NOT_RUN|
|T14|인덱서 지연·WS 단절·잠정 체결 정정|신선도·확정 상태가 UI에 그대로 표시|Wallet / Settlement·Exchange|NOT_RUN|synthetic 상태 schema/fixture 및 브라우저 crypto만 PASS; 실제 REST/WS·정정 UI NOT_RUN|
|T15|모듈 계정 먼지 유입·잘못된 mint/burn|초과액 격리·정상 출금 유지; 부족액/무단 발행 중지|Chain / Settlement·Security|NOT_RUN|실제 모듈 bank/supply/mint/burn 경로 NOT_RUN|
|T16|앞 배치 거절 뒤 같은 주문·잔고를 쓰는 후속 잠정 체결|의존 체결 연쇄 무효화·예약 재계산·새 호가 스냅샷 일치|Exchange / Settlement·Wallet|NOT_RUN|모의 정정 선행 조건 PASS; 의존 폐쇄·예약·WAL replay NOT_RUN|

증거: `security/evidence/differential.json`, `go-tests.jsonl`, `rust-tests.txt`, `ts-run.txt`, `settlement-tests.txt`, `settlement-independent.json`, `qa/evidence/browser.json`와 보고서의 동일 SHA manifest. 부분 시험 성공은 원문 전체 PASS가 아니다. T05는 공통 장부 검증이며 v1 현물 시장을 추가하지 않는다. 실제 자산 보존·음수/중복 지급·ACK 누락 0을 아직 검증하지 않았다.

2026-09-29 rc4 재인수: NUS-9 현재 리비전 3개가 위 기록과 동일함을 API에서 확인했다. 이번 원시 증거는 qa-rc4/evidence/local-g, local-b, browser와 audit.json이다. 원문 번호·시나리오·통과 조건·담당자는 유지하며 rc4 부분 PASS만 갱신한다. 제품 판정 16개는 모두 NOT_RUN이다.
