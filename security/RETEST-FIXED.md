# S0-G 수정 후 전체 독립 재시험

로컬 783비교 중 782일치·1차이. 기존 G-RC3-01/02의 5차이는 해소됐고 enum 정상 네 조합 및 epoch 네 조합은 모두 통과했다. 실제 ML-DSA 27 교차검증 및 codec/domain/context 각 27 비교 PASS. 실제 Chrome 392 conformance와 키 생성·서명·메모리 복구·390px UI 검사 PASS.

G-FIX-01 (Low, 공통 출력 일치 문제): 서명과 나머지 snapshot은 정상이고 epoch_matches만 누락하면 Rust는 snapshot 전체를 null로 교체하여 snapshot_id=null을 반환한다. Go/TS와 기존 기대는 synthetic-1이다. 세 구현 모두 인증 PASS/정책 NOT_CONNECTED/ACK NOT_CONNECTED이며 승인 우회나 실제 자산 공격은 재현되지 않았다. 원래 업무 NUS-13, SHA 7f9f14f35a985484cee64a5132ed26aa500c0887, exchange/src/decision.rs의 binding 처리. 원시 ID rc3-signed-missing-epoch_matches. 담당 Exchange, 의미 확정 CTO. 재시험은 누락/정상/모순 epoch와 snapshot_id 전체 출력 교차 비교.

계약은 snapshot 누락 시 ID null이라고 명시하지만 일부 필드 누락의 ID 보존을 별도로 확정하지 않는다. 따라서 새 보안 High나 이미 확정된 규약 위반으로 분류하지 않는다. 공통 출력 차이는 남아 있어 전체 일치 게이트 FAIL, 시험한 보안 거절 불변식 PASS로 구분한다. 임의 기대값 변경이나 구현 수정은 하지 않았다.

Linux CI 재현 진행 중. 최종 제출 전 실행 URL과 원시 artifact 대조 결과를 추가한다. 이전 rc2 420/413/7 및 rc3 759/754/5 FAIL은 보존한다. 실제 ACK/WAL/원장/체인/REST/WS 및 제품 T01~T16은 NOT_RUN/NOT_CONNECTED. 검토 완료는 제품 PASS·출시 승인과 다르다.
