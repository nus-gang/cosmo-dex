# G-RC3-01 Wallet enum 정책 수정

2026-09-29 · [NUS-15](/NUS/issues/NUS-15) · PR #6 후속 수정

`decideOrder`는 실제 인증과 snapshot 결합·정책이 통과한 뒤 서명된 `side` 및 `order_type`이 각각 1 또는 2인지 검사한다. 그 외 값은 인증 PASS를 유지하고 정책 REJECTED/MARKET_LIMIT로 반환한다. 기존 fee/cap/키/epoch 검사 순서와 실패를 보존한다. ACK NOT_CONNECTED, WAL NOT_RUN, ledger NOT_CONNECTED는 변하지 않는다.

정상 enum 4조합·비정상 5조합을 실제 ML-DSA로 서명해 Node 및 실제 Chrome 공통 suite에서 검사한다. 392 assertions PASS(기존 347 + 새 45), build/typecheck PASS. 공통 contract/vector hash는 아래 값 그대로이며 protocol 파일은 변경하지 않았다. 기존 공개 decision 벡터 21개와 합성 fixture 34개 재생성 결과는 rc3와 동일하다.

- contract: `a71a8c03fea5e4d2876612e821eafcb4b359a0b132157929b9924d6f8fecd73e`
- vectors: `4851d9d674b2412ca8919d8347a71da13f9adf4426fe60b44e2a4a259f8bd948`
- 동일 요청 38개: Go 38/38, 수정 TS 38/38, **기존 Rust 30/38**. Rust enum 7비교·epoch 1비교 실패를 보존한다. 정상 1/2 네 조합은 세 언어 모두 일치한다.
- 비교 입력 hash: `60eae93f3fe0eee072605a2053c0e53eb50efcb5082839dad8d7742d53fd6766`

교차 비교는 Security `068477c` checkout의 기존 Go/Rust binary를 읽기 전용으로 소비했다. 각각 Go 제출 `5d39b7a`, Rust 제출 `d298422` 기준이다. binary hash는 summary.json에 기록한다. 새 Rust 수정본이나 fresh build/설치의 검증이 아니다. Security의 `runner.ts`·공개 `policy-inputs.json`에서 복사한 입력/adapter는 이 Wallet checkout에 격리했다. 기존 signed fee/cap/등록키/epoch/snapshot 결합 사례 29개와 새 enum 9개를 비교하며 전체 독립 보안 매트릭스를 대체하지 않는다. mock 개인키는 저장하지 않고 공개 synthetic seed만 사용한다.

## 재현

Wallet checkout에서 기존 고정 의존성 설치 후:

```sh
npm --prefix web test
npm --prefix web run build
(cd web && EVIDENCE_DIR=evidence/enum-fix npm run test:browser)
python3 protocol/v1/tools/check.py
node --experimental-strip-types web/scripts/decision-vectors.ts > /path/to/decision-results.json
cmp web/evidence/rc3/decision-results.json /path/to/decision-results.json
python3 web/test/cross-enum/compare.py /path/to/security-068477c /path/to/results
```

Security checkout 준비는 해당 `security/README.md`·`prepare.py`·`run.sh`에 따른다. 비교 명령은 기존 Rust 실패 때문에 **exit 1**을 유지한다. 이를 전체 PASS로 해석하지 않는다. 상세 원시 결과·브라우저 스크린샷·시험 SHA는 Paperclip artifact로 제공한다.

기존 Security **759비교/754일치/5차이, 보안 FAIL/암호 상호운용 PASS**는 유지한다. G-RC3-02 Rust 구현은 변경하지 않았다. 새 CTO→Security 개별 검토 후 CTO가 C/D/F SHA를 고정하여 [NUS-16](/NUS/issues/NUS-16) 전체 독립 재시험을 조정한다. [NUS-17](/NUS/issues/NUS-17)의 fresh checkout/공통 CI 인수도 별도다. REST/WS·체인·영속 백업은 미연결/미구현이며 main 병합·후속 기능·출시는 승인하지 않는다.
