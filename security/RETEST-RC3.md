# S0-G rc3 독립 재시험 — FAIL

2026-09-29 · Security · [NUS-16](/NUS/issues/NUS-16)

수정 후 보안 verdict는 **FAIL**, 실제 ML-DSA Go/Rust/TS 상호운용은 **PASS**다. 기존 420비교·7차이/FAIL 기록은 `REVIEW.md`, `evidence-rc2/` 및 기존 Paperclip 문서·ZIP에 보존했다. 이번 759비교 중 754일치·5차이는 별도 기준선이다. 검토 완료는 제품 출시 승인과 다르다.

## 입력·재현

|입력|승인 SHA|
|---|---|
|A / CTO|549ce150d6a9f21ec30f159d39a4d91c31dbd759|
|C / Chain [NUS-12](/NUS/issues/NUS-12)|5d39b7a0ffe911ed60cd0e3a56ada135e76a425f|
|D / Exchange [NUS-13](/NUS/issues/NUS-13)|d2984223fac6c9dfafc0d71ab9f86af5bf539339|
|E / Settlement [NUS-14](/NUS/issues/NUS-14)|f9b5bbf1e06a4a0c43bebc6d5bd081dc3b1892c2|
|F / Wallet [NUS-15](/NUS/issues/NUS-15)|8a70632d933b58c15b5bbac9fbef25dfa7312643|

API의 done/approved와 각 commit work product를 직접 대조했다. C/D/F protocol 40파일씩은 A rc3와 byte-for-byte 동일하다. E의 원래 rc2 manifest는 유지한다. E가 실제 소비하는 `schema.json`, `vectors/message-codec.json`, `vectors/s0-cases.json`, `vectors/batches.json` 네 파일은 rc3와 동일하며 E 시험을 rc3 입력으로 재실행했다. E 전체 manifest가 rc3라고 표시하지 않는다. 원래 구현 파일은 변경하지 않았고 새 시험 adapter만 별도 `security/`에 작성했다.

- contract: `a71a8c03fea5e4d2876612e821eafcb4b359a0b132157929b9924d6f8fecd73e`
- vectors: `4851d9d674b2412ca8919d8347a71da13f9adf4426fe60b44e2a4a259f8bd948`
- config: `7b12f8dffd4dfd07242331b975f0c440f5948074280c3a120c3232b3e674d13e`

새 checkout에서 `python3 security/prepare.py` → `bash security/run.sh`. 실패 기대값을 무시하지 않으며 최종 differential이 exit 1을 반환한다. 원시 `differential.json`, `policy-inputs.json`, `generated.json`, 구성요소 로그를 첨부한다. harness도 Security 작성물이므로 CEO 검토와 H 독립 재현을 받는다.

## 결과와 이전 findings

- 기존 420개 비교가 모두 일치한다. G-04는 새 rc3 인증/정책 분리 포트에서 q=p=1, active=cap=25에 인증 PASS·정책 FEE_GE_RECEIVE·ACK NOT_CONNECTED를 확인했다. 옛 부분 validator의 OK를 주문 승인으로 바꾸지 않았다.
- G-01: 정상/OTHER/미등록/다른 bytes/누락 타입 및 변조 서명을 실제 최상위 함수에 전달해 세 언어 일치. 고정 SHA에서 재현 사례 수정 확인.
- G-02: rc3 17 fee 사례, U128 최대·overflow, bps 비정규/범위, receive 선검증 일치.
- G-03: rc3 21 cap 사례 및 cap 0/25/10000/10001/U32_MAX를 새로 서명한 주문 검사 일치. 승인 rc3 규약을 기준으로 한다.
- G-04: 34 합성 정책 벡터 및 실제 인증과 결합한 revoked/ID/잔고/누적량/만료/누락 상태 시험을 실행했다. 기존 재현은 수정 확인했지만 아래 새 결함 때문에 판정 포트 전체는 FAIL이다.
- 암호 3×3 각 칸 Order/Cancel/Wallet 3건, 총 27 검증 PASS. 다른 공개 synthetic seed로 실제 9개 서명 생성. 27 codec/frame, 27 domain 변조 거절, 27 FIPS context 교차 거절도 PASS. 공통 3 positive/35 negative fixture를 유지했다.
- Go 제출 계약 시험, Rust 16 integration tests, TS Node 347 assertions 및 build/typecheck PASS. 정산 기존 16 tests와 독립 20조건 PASS. 이번 브라우저 재시험은 NOT_RUN(이전/개별 승인 브라우저 결과와 분리).
- 759는 비교 수이며 고유 취약점 시험 수가 아니다. 추가 339비교 중 5차이이며 동일 원인 두 findings에 대응한다.

## G-RC3-01 — 새 판정 포트의 주문 enum 검증 누락

**Medium / FAIL**, 담당 Exchange [NUS-13](/NUS/issues/NUS-13), Wallet [NUS-15](/NUS/issues/NUS-15). 위 D/F SHA의 `exchange/src/decision.rs::admit_order`, `web/src/decision.ts::decideOrder`.

재현: 정상 DEV q=p=100, cap=active=25에서 `side=3` 또는 `order_type=3`으로 바꾸고 모의 키로 새 서명한다. 등록 계정·snapshot id/height·서명은 유효하다. `rc3-signed-side-invalid`, `rc3-signed-order-type-invalid`의 JSON 요청을 사용한다.

기대: 인증 PASS 후 정책 REJECTED/MARKET_LIMIT. 실제: Go는 기대대로 거절; Rust와 TS는 정책 PASS/OK. 합계 4차이. 기존 Rust `validate_order`는 enum을 검사하지만 새 `admit_order`는 `authenticate_order`와 제한된 snapshot 검사만 호출한다. TS도 새 정책 함수에서 side/order_type을 검사하지 않는다. 같은 유효 서명에 대한 정책 차이가 있으며 실제 ACK나 자금 이동은 없다.

조치: 원래 구현 담당자가 새 판정 경계에서 서명된 주문의 enum·market 규칙을 기존 규칙과 일치시킨다. 검증: 위 두 부정 입력 및 side/order_type 1/2 긍정 입력을 세 언어로 비교하고 공통 벡터 회귀를 재실행한다. 이 리뷰에서 구현을 수정하지 않았으며 수정 후 재시험 NOT_RUN.

## G-RC3-02 — Rust의 epoch 관측과 합성 flag 결합 누락

**High(통합 조건부) / FAIL**, 담당 Exchange [NUS-13](/NUS/issues/NUS-13), CTO 공통 판정 경계 검토. D SHA `d2984223fac6c9dfafc0d71ab9f86af5bf539339`, `exchange/src/decision.rs::admit_order`.

재현 `rc3-epoch-flag-binding`: 정상 서명된 owner_epoch를 유지하고 신뢰 context `Epoch=999`로 바꾼다. snapshot은 `epoch_matches=true`를 유지한다. 인증은 epoch 정책을 분리하므로 PASS가 가능하나, 정책은 관측과 모순되는 flag를 받아 PASS하면 안 된다.

기대: 정책 REJECTED 또는 NOT_CONNECTED(모순 snapshot의 세부 오류 코드는 rc3에 표준화되지 않아 특정 코드 일치를 강요하지 않는다). 실제: Go CONTEXT_MISMATCH, TS NOT_CONNECTED, Rust PASS/OK. Rust `admit_order`의 결합 목록에는 q/p/cap/expiry/id/height만 있고 owner_epoch 대 ctx.epoch와 epoch_matches의 결합이 없다. 원래 `validate_order`에는 epoch 비교가 있다.

위협 조건: 이후 adapter가 오래되었거나 모순된 epoch_matches를 공급하면서 새 포트 PASS를 사용하면 무효화된 epoch 주문의 정책을 통과시킬 수 있다. 현재 입력은 합성 snapshot이며 외부 요청자가 실제 확정 상태를 조작했다거나 실자산 공격에 성공했다는 증거가 아니다. ACK는 계속 NOT_CONNECTED다.

조치: 원래 담당자가 인증된 owner_epoch와 같은 관측 context에서 flag의 일관성을 검사한다. 검증: 정상 일치, 실제 epoch 불일치/false 거절, epoch 불일치/true 모순 거절, epoch 일치/false 모순 거절을 모두 재실행한다. 수정 후 재시험 NOT_RUN.

## 범위·인수

실제 자산 보존·음수 잔고/중복 지급·출금/정산 경합·후원 우회·직접 회수·영속 ID/ACK/WAL replay는 NOT_RUN. REST/WS/chain/DB/원장은 NOT_CONNECTED. 잠정액 비가용·timeout 보류·과거 receipt·동일 bytes 재시도는 모의 정산 adapter 수준만 PASS다.

재사용 소스는 기존 고정 lock(CIRCL 1.6.3, fips204 0.4.6, noble 0.4.1, hashes 1.8.0, scure/base 1.2.6, OrderBook-rs 0.13.1)을 유지한다. 재다운로드한 소스/라이선스와 잠금 hash를 확인하며 세부 기존 출처·제한은 `REVIEW.md`에 보존한다. 독자 암호 설계, 전체 SBOM/취약점 전수 감사 또는 법률 승인은 없다.

CEO는 부정 판정 산출물을 인수하고 수정은 원래 D/F 업무로 반환한다. CTO에게 같은 findings를 공유한다. [NUS-17](/NUS/issues/NUS-17)은 G의 done을 PASS로 해석하지 않고 이 새 verdict와 별도 fresh checkout/공통 CI 결과를 읽어야 한다. main 병합·후속 기능·출시는 승인하지 않는다.

## CI

실행 링크·시험 SHA·artifact 대조 결과는 실행 완료 후 아래에 기록한다. 아직 결과가 없는 상태를 PASS라고 쓰지 않는다.
