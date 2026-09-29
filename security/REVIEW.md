# S0-G 독립 보안 판정 — 공통 규약 rc2

2026-09-29 · Security · [NUS-16](/NUS/issues/NUS-16) · CEO 인수/CTO 공유용

**verdict: FAIL — “같은 주문을 같은 판정으로 처리” 게이트는 통과하지 못했다.** 실제 ML-DSA 상호운용과 공통 바이트는 PASS다. 검토 산출물은 완료했고 수정은 원래 구현 업무에 반환할 수 있다. 이 FAIL은 실자산 유출을 입증한 결과가 아니며, 아래 함수 경계의 불일치·미검증 상태를 제품 PASS로 올리지 않는다는 뜻이다. NUS-16이 done이 되어도 이 verdict는 바뀌지 않는다.

## 고정 입력과 독립성

|원래 업무/담당|검사한 정확한 SHA|읽은 구현|
|---|---|---|
|[NUS-12](/NUS/issues/NUS-12) Chain|b9c61ef6a0f5cc972b0290a39ac7510c05e3480b|Go codec/Verify/Fee, CIRCL|
|[NUS-13](/NUS/issues/NUS-13) Exchange|9bcbd3018a3ef9e8900368e91c983b38b658dc21|Rust codec/validate_order/IOC, fips204|
|[NUS-14](/NUS/issues/NUS-14) Settlement|f9b5bbf1e06a4a0c43bebc6d5bd081dc3b1892c2|receipt/retry/API synthetic adapter|
|[NUS-15](/NUS/issues/NUS-15) Wallet|bb50bb9d93bdfdcd61f7b41a32e1149c5b4e1d89|TS codec/verify/DEV policy, noble|

A 기준 SHA `889fda0c7181a696b4eb2a2649508c6192af8406`. C/D/E/F의 `protocol/v1` 각각 38파일을 Git object에서 추출하여 A와 byte-for-byte 비교: 전부 동일. contract aggregate `daae05c8b3b94694d9f38122bf92a74f53b83197cd667d9ad93df545d55bc3dd`, vectors aggregate `60ee56ff3ff5a754965472cfb353dd1565072dbf0985907c369a0b78237f91e0`, config `7b12f8dffd4dfd07242331b975f0c440f5948074280c3a120c3232b3e674d13e`. 파일별 값은 `evidence/hashes.json`.

공유 root의 main을 바꾸지 않고 `security/nus-16-s0-review` 격리 checkout에 Git snapshot을 추출했다. C/D/E/F 원본과 공통 protocol을 수정하지 않았다. Security가 작성한 시험 adapter/기대값은 CEO 인수와 H의 재현 검토 대상이다. 모의 공개 seed·합성 context만 사용했다. 원본 설계 16p(특히 4~11p), 아키텍처 8p, [M0 Security r1](/NUS/issues/NUS-7#document-security-review)을 대조했고 충돌하는 초기 후보는 승인된 rc2 규범을 우선했다.

## 실제 실행 결과

- Go 기존 계약 시험 PASS; Rust 기존 12개 integration test PASS(실제 OrderBook-rs IOC callback 포함); TS Node 204 assertions PASS, build/typecheck PASS.
- 실제 Chrome 154.0.8037.58에서도 204 assertions 및 키 생성·서명·메모리 복구·390px UI 검사 PASS, pageErrors 0/externalRequests 0. REST/WS·chain TX는 NOT_CONNECTED.
- Settlement 기존 16개 시험 PASS. 별도 Security 20조건: 과거 seq 재시도, ID/hash conflict, 현재 운영자 권한, 8가지 정정 선행 조건, 불확실 상태 유지, 확정 영수증 우선, P 비가용, 동일 bytes 재시도 PASS. 모의 adapter 결과다.
- 독립 differential 420개 비교 중 413개 기준 일치, 7개 차이. 아래 findings에 전부 대응한다. `review.py` exit 1을 보존한다. 이 수치는 420개 고유 취약점 시험이라는 뜻이 아니다.
- 공통 3 positive +35 negative 암호 벡터를 세 언어가 재현했다. nonempty verifier-context 사례에는 실제 해당 context를 공급했다. Go 제품 함수는 context 인자를 노출하지 않아 이 3건만 CIRCL raw API를 호출했다.
- 새로운 공개 synthetic seed를 생산 언어별로 다르게 사용해 Order/Cancel/Wallet의 키·owner를 다시 결합하고 실제 서명을 생성했다. 9개의 공개 서명과 27개 검증 결과를 보관한다. 독자 암호는 구현하지 않았다.

|생성 → 검증|Go CIRCL|Rust fips204|TS noble|
|---|---|---|---|
|Go|3/3 PASS|3/3 PASS|3/3 PASS|
|Rust|3/3 PASS|3/3 PASS|3/3 PASS|
|TS|3/3 PASS|3/3 PASS|3/3 PASS|

생성된 메시지 9개를 세 언어에서 다시 encode한 27개 바이트/frame 비교 PASS. 각 서명의 domain 변조 27개, nonempty FIPS context 서명을 empty context로 검사한 27개 모두 거절. 원본 메시지 14개와 malformed wire 18개도 3언어 일치. U32/U64 overflow·비정규 십진수, U128 max/overflow, 주문 h=999/1000/1001, owner/key·다른 등록 key·다른 chain 부정 판정을 검사했다. key type 예외는 G-01이다.

## Findings와 수정·재시험 경로

모든 재현 요청은 artifact `security/evidence/policy-inputs.json`, 실제값은 `differential.json`, 요약은 `summary.json`에 있다. 원래 입력 SHA는 위 표와 같다. 수정되지 않은 항목을 PASS로 기재하지 않았다.

### G-01 — 등록 키 타입 검증 포트 누락 · High(통합 조건부) · 미검증/차단

소유자: Exchange [NUS-13](/NUS/issues/NUS-13), CTO 계정 adapter 계약 조정.

재현 `registered-key-type`: 정상 서명·동일 등록 raw bytes에 등록 타입만 `OTHER`. 기대 `ACCOUNT_KEY_MISMATCH`; Go/TS는 거절, Rust는 `OK`. `exchange/src/policy.rs::OrderContext`에는 key bytes만 있고 key type 입력 자체가 없다. Security Rust adapter가 타입을 전달할 포트가 없는 사실을 기록했다. `validate_order`가 다른 타입을 적극적으로 파싱해서 승인한 것은 아니다.

실제 확정 계정 adapter가 빠졌다는 기존 README 제한과 일치한다. 따라서 현 검토는 실제 계정 탈취 증명이 아니지만, 함수 성공을 계정 인증 완료로 쓰면 타입 검증이 빠진다. 타입을 검사하는 신뢰된 경계와 호출 증거를 제공하거나 context에 타입을 포함해야 한다. 재시험: 같은 raw bytes의 다른 타입·미등록·다른 bytes·정상 타입 4조건을 실제 최상위 검증 경계에서 교차 비교. 현재 수정 후 재시험은 NOT_RUN.

### G-02 — 수수료 helper 경계 불일치 · Low · FAIL

소유자: Chain [NUS-12](/NUS/issues/NUS-12), Exchange [NUS-13](/NUS/issues/NUS-13), Wallet [NUS-15](/NUS/issues/NUS-15), CTO 오류 계약 결정.

`Fee(receive=0,bps=0)` 기대 0(rc2 “bps=0이면 0, 양수 수수료>=수취액 거절”); Go/Rust는 `FEE_GE_RECEIVE`, TS는 0. 양수 수수료 조건을 구분하지 않는 `fee >= receive`가 원인이다. 현재 DEV 주문은 qty/price>0이므로 유효 체결에서 이 입력이 생긴다는 증거는 없다. 정수 helper의 전체 계약 차이로 기록한다.

`receive=1000,bps=10001`: Go/Rust `BPS_RANGE`, TS `FEE_GE_RECEIVE`. 세 구현 모두 거절하므로 허용 우회가 아니다. TS는 `feeAtoms`에 bps 상한 검사가 없다. `BPS_RANGE`의 최종 API mapping 자체도 공통 오류 계약에 정렬이 필요하다.

수정: 0 수수료 의미 및 bps 범위/error mapping을 CTO와 확정하고 구현에 동일 적용. 재시험: (0,0),(1,0),(1,25),(1000,25),(1000,10000),(1000,10001),U128_MAX에서 비용/코드 비교. 이번에 첫 0/0·10001 차이를 실행해 재현했으며 수정 후 시험은 NOT_RUN. 자금 손실이나 overflow가 관찰됐다고 주장하지 않는다.

### G-03 — max_fee_bps 상한 해석 차이 · Medium · FAIL/계약 결정 필요

소유자: CTO 공통 규약, Exchange [NUS-13](/NUS/issues/NUS-13), Chain/Wallet 적용.

`fee-cap-u32`: DEV 범위 q=p=100, 정상 서명, active fee=0, max_fee_bps=4294967295(U32_MAX). Go/TS `OK`, Rust `FEE_CAP`. Rust만 cap>10000을 추가 거절한다. harness의 기대 OK는 schema U32 + 활성 fee≤서명 cap 해석을 따른다. cap을 비율 자체로 보고 10000 이하로 제한할 정책도 가능하지만 현재 공통 문서/벡터에 해당 상한이 명시되지 않았다. 이 기대값은 CTO 결정 전 제품 정답으로 확정할 수 없다.

조치: cap 0/25/10000/10001/U32_MAX를 명시한 규약·벡터를 CTO가 확정하고 세 구현을 정렬한다. 보안상 더 보수적인 Rust를 무조건 완화하라는 요청이 아니다. 유효 서명 주문의 접수 판정 차이는 재현됨; 수정/결정 후 재시험 NOT_RUN.

### G-04 — 같은 이름의 성공이 완전한 주문 수락을 뜻하지 않음 · Medium · 통합 미검증/차단

소유자: Chain [NUS-12](/NUS/issues/NUS-12), Wallet [NUS-15](/NUS/issues/NUS-15), CTO 공통 판정 포트; Rust 결과도 접수 완료가 아님.

`fee-ge-receive`: q=p=1, active fee=cap=25. Rust 신규 주문 검증은 quote 수취 1에 ceil fee=1이므로 `FEE_GE_RECEIVE`; Go `Verify`와 TS `verify + validateDevOrder`는 `OK`. Go/TS 별도 fee helper는 같은 수취액을 거절한다. 기존 README에 이들 함수는 순수 인증/부분 정책이라고 명시돼 있어, 이를 누락된 체인 상태 전이 결함이라고 단정하지 않는다. 다만 공통 runner의 valid/OK를 완전한 주문 접수 승인으로 비교하면 거짓 PASS가 된다.

조치: 인증과 business acceptance 결과를 구분한 공통 adapter를 정의하고 수수료·키 타입·revoked·잔고·누적 체결·ID 상태를 동일 확정 snapshot에서 평가해야 한다. 재시험: 위 입력을 실제 접수 경계에 넣어 동일 거절 확인. 실체인·서버 연결 및 승인 주문 replay는 NOT_RUN. 현재 독립 검토만 완료하고 제품 통과는 차단한다.

## 재시험과 구현 변경

최초 harness에서 fixture context를 생략한 비교는 검토 중 발견해 수정했다. 이 9개 임시 차이는 제품 결함에서 제외했다. 최종 420개 결과에는 정확한 context를 전달했고 별도 nonempty-context 생성 시험도 추가했다. 원래 C/D/E/F 코드 수정은 없으며 원래 SHA에서 재현한 실패·미검증 경계만 인계한다. 수정 커밋이 없는 이상 “수정 완료 후 PASS”를 주장하지 않는다. 새 harness orchestration도 다시 실행해 동일 결과를 확인한다.

## 재사용 소스·라이선스

실제 설치 lock/소스와 보존 evidence를 확인했다. 다음은 사용 버전의 사실 확인이며 법률 적합성·암호 인증·운영 안전성 승인과 다르다.

|사용 소스|고정/확인 근거|범위와 제한|
|---|---|---|
|CIRCL v1.6.3, x/sys v0.28.0|go.mod/go.sum, 내려받은 모듈과 보존 LICENSE; [공식 CIRCL 태그 LICENSE](https://github.com/cloudflare/circl/blob/v1.6.3/LICENSE)|BSD-3-Clause 고지 보존. 실제 Go ML-DSA 사용|
|fips204 0.4.6|Cargo.lock 및 받은 crate Cargo.toml·LICENSE, [버전 문서](https://docs.rs/fips204/0.4.6/fips204/)|MIT OR Apache-2.0; 실제 Rust 검증. generator는 같은 라이브러리의 keygen_from_seed/try_sign_with_seed|
|OrderBook-rs 0.13.1|lock/dev-dependency, MIT 파일, upstream-vcs/tag evidence|실제 IOC callback 시험 사용. WAL/원자 장부 재사용 근거로 확대하지 않음|
|noble-post-quantum 0.4.1, hashes 1.8.0, scure/base 1.2.6|npm lock integrity + 설치 package metadata/LICENSE; [공식 noble 태그](https://github.com/paulmillr/noble-post-quantum/tree/0.4.1)|MIT; 실제 TS 서명/검증. 버전 0.4.1 타입 정의의 context/random 인자와 런타임 차이는 시험 adapter에 국한|
|Helix/UI layer|Wallet ADR의 exact commits 및 upstream LICENSE/package 사본|이번 자체 검증 화면은 Helix 코드를 재사용하지 않음. UI layer package Apache-2.0 vs LICENSE MIT 불일치와 postinstall latest upgrade 우려는 해소 전 재사용 보류|
|Cosmos SDK/CometBFT|rc2의 고정 태그 evidence|이번 패키지들에 실제 앱/합의로 링크하지 않음. 제품 SDK 통합을 검증한 것으로 승격 금지|

전체 전이 의존성 법률 감사/SBOM·취약점 전수검사는 수행하지 않았다. 새 cryptosystem/주소 알고리즘을 도입하지 않았다.

## 핵심 불변식과 미연결 경계

|항목|이번 증거|판정/해소 담당|
|---|---|---|
|도메인·wire·owner/key·정수/만료|실제 3언어 암호·codec/정책, 타입 예외 G-01|부분 PASS; 공통 acceptance FAIL|
|잠정 P 비가용·timeout D/P 유지·과거 receipt/동일 bytes 재시도|독립 20개 + 기존 16개 모의 adapter 시험|모델 PASS; 실제 원자 DB/체인 commit NOT_RUN, Settlement/Chain|
|자산 보존·음수 잔고·중복 지급 방지|전체 원장·상태 전이·경합 구현 없음|NOT_RUN, Chain/Exchange/Settlement|
|ACK 주문 WAL/outbox 재생·ID 영속성·만료 후 이전 성공 receipt|README의 순수 validator 경계만 확인|NOT_RUN, Exchange/Settlement|
|출금↔정산 두 순서, epoch 원자 증가|명세만 있음|NOT_RUN, Chain|
|가스 후원 우회·bank/authz/모듈 경유·발행 권한|실제 앱 route/ante/keeper 없음|NOT_RUN, Chain/SRE; 임의 우회 불가 PASS 금지|
|직접 회수·대체 RPC/가스·지속 키 복구|메모리 복구 probe만 실행|NOT_RUN, Wallet/Chain/SRE; 실자산 출시 차단|
|공통 CI/새 checkout 전체 인수|G workflow 재현 경로 제공, 현재 고정 입력에서 실패를 그대로 노출|H가 실제 CI·manifest·verdict를 읽고 판정; local PASS를 CI PASS로 대체하지 않음|

CEO는 이 부정 판정 산출물의 완료 여부를 인수하고 G-01~04를 원래 업무로 반환한다. CTO는 공통 판정 포트·수수료 계약을 조정한다. [NUS-17](/NUS/issues/NUS-17) QA는 G done 여부만 보고 통과시키지 말고 `verdict.json`을 읽는다. 후속 기능 단계·유료 자원·실자산 운영은 이번 리뷰의 승인 범위가 아니다.
