# G-FIX-01 — 부분 snapshot 관측 ID와 binding 출력 (rc4)

2026-09-29 · CTO · Security→QA 새 회차 검토 후보. 독립 최종 검증 전.

## 근거와 선택

rc3 SHA `549ce150d6a9f21ec30f159d39a4d91c31dbd759`는 snapshot 전체 누락 시 null만 명시했다. 부분 필드 누락에서 ID 보존은 확정하지 않았다. Security의 783/782/1, 공통 출력 FAIL, 시험한 인증·거절/암호 PASS 및 G-FIX-01 Low를 보존한다. 과거 Rust 출력은 새 결정의 소급 위반으로 분류하지 않는다.

`snapshot_policy.snapshot_id`를 **입력 snapshot에 관측된 식별자의 진단용 값**으로 정한다. 완전한 정책 입력, context와의 결합 성공, 인증 성공 또는 승인 증거가 아니다. 부분 입력·모순 입력을 추적할 때 실제 받은 ID를 유지하는 편이 원인 분석에 유리하다. 완전한 정책 입력의 ID로 한정하는 대안은 null만 보고 전체 누락과 부분 누락을 구분할 수 없어 채택하지 않았다. context의 ID를 대신 넣으면 입력 출처의 모순이 숨겨지므로 금지한다.

## 규범 (rc3 출력 설명의 모호한 부분을 보완)

1. 이 포트는 신뢰된 adapter가 공급하는 typed snapshot object/null을 받는다. ID는 string/null/누락이다. ID가 문자열이면 빈 문자열까지 그대로 출력한다. snapshot 자체 또는 id가 누락/null이면 출력 null. 잘못된 JSON 타입의 parser 오류는 이 typed 포트 밖이며 PASS로 변환하지 않는다.
2. 출력 ID는 원본 입력에서 먼저 얻어 보존한다. 부분 필드 누락, binding 실패, 인증 거절에서도 같은 규칙이다. context ID로 채우거나 binding 실패 때문에 snapshot 전체를 null로 덮어쓰지 않는다. source=SYNTHETIC은 시험 경계 표시이며 입력 출처가 검증됐다는 뜻이 아니다.
3. authentication은 실제 인증 함수의 독립 결과를 그대로 둔다. 인증이 PASS가 아니면 snapshot_policy는 NOT_RUN/code=null이다. ID는 1번대로 유지한다.
4. 인증 PASS 후 rc3의 13개 필수 필드 중 하나라도 누락/null, 빈 ID, 미연결 context(snapshot_id/height/epoch 누락/null 또는 빈 snapshot_id)이면 정책 NOT_CONNECTED/code=null. 0/false/무한 잔고 기본값을 만들지 않는다. rc3의 source/flag/id_state 연결 유효성도 유지한다.
5. 완전한 snapshot도 동일 관측·서명에 결합되어야 한다. snapshot.id=context.snapshot_id, height=context.height, q/p/cap/expiry_height=인증된 주문 max_qty_lots/limit_price_ticks/max_fee_bps/expiry_height, epoch_matches=(주문 owner_epoch=context.epoch)를 모두 요구한다. 모순이면 정책 NOT_CONNECTED/code=null로 통일한다. 이 단계는 금액/ID_CONFLICT 등 정책 실행보다 앞선다. 신뢰 context가 빠지거나 모순인 것은 사용자의 유효한 정책 거절로 포장하지 않는다.
6. 결합이 일치하면 기존 rc3 정책 순서를 적용한다. 실제 epoch가 다르고 flag=false인 일관된 입력은 EPOCH_MISMATCH 거절(선행 ID_CONFLICT가 있으면 그 오류). epoch가 다른데 flag=true 또는 같은데 flag=false면 binding 미연결이다. q=p=1, active=cap=25는 계속 FEE_GE_RECEIVE 거절이다.
7. 모든 결과에서 ack=NOT_CONNECTED, wal_replay=NOT_RUN, ledger=NOT_CONNECTED. ID 보존이나 합성 정책 PASS를 ACK·자금 예약·원장 반영으로 해석하지 않는다.

## 벡터·실행 경계

`vectors/snapshot-output.json`의 60개 새 ID가 정상, 전체 누락/null/빈 object, 13개 필드 각각 누락/null, 6개 binding 모순, epoch 4조합, context 6개 누락/null 및 빈 ID, 인증 거절/미연결, 부분+모순, 수수료·오류 우선순위의 전체 출력을 고정한다. 모든 expected는 authentication/snapshot_policy/ack/wal_replay/ledger를 포함한다.

입력 authenticated_order는 인증된 주문의 binding 관련 전체 projection이며 authentication_result와 함께 합성 주입값이다. 제품 시험은 기존 모의 OrderV1 fixture에 이 필드를 적용해 재직렬화·재서명하고 실제 인증 결과로 대체해야 한다. 기존 signature를 변조한 채 PASS를 주입해서는 안 된다. Python 예제는 제품 암호 증거가 아니다.

기존 rc3 vectors 파일은 변경하지 않는다. 기존 Security 783개 ID/언어 및 기존 FAIL 로그를 보존하고 새 60개 명세 사례는 별도 집계한다. 60×3 제품 결과를 실행 전에 PASS/총 비교 수에 더하지 않는다. 기존 모순 사례의 불변식 비교를 전체 출력 비교로 강화할 때 ID를 유지하고 새 기준 적용을 명시한다.
