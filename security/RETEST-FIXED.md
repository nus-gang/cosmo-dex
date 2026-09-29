# S0-G 수정 후 전체 독립 재시험 — 공통 일치 FAIL

보안 검토 verdict는 공통 출력 일치 조건 미충족으로 **FAIL**을 유지한다. 실제 인증·거절 불변식 및 ML-DSA 상호운용은 시험 범위에서 **PASS**다. 남은 Low 차이를 인증 우회로 해석하지 않는다.

로컬 783비교 중 782일치·1차이. 기존 G-RC3-01/02의 5차이는 해소됐고 enum 정상 네 조합 및 epoch 네 조합은 모두 통과했다. 실제 ML-DSA 27 교차검증 및 codec/domain/context 각 27 비교 PASS. 실제 Chrome 392 conformance와 키 생성·서명·메모리 복구·390px UI 검사 PASS.

G-FIX-01 (Low, 공통 출력 일치 문제): 서명과 나머지 snapshot은 정상이고 epoch_matches만 누락하면 Rust는 snapshot 전체를 null로 교체하여 snapshot_id=null을 반환한다. Go/TS와 기존 기대는 synthetic-1이다. 세 구현 모두 인증 PASS/정책 NOT_CONNECTED/ACK NOT_CONNECTED이며 승인 우회나 실제 자산 공격은 재현되지 않았다. 원래 업무 [NUS-13](/NUS/issues/NUS-13), SHA 7f9f14f35a985484cee64a5132ed26aa500c0887, exchange/src/decision.rs의 binding 처리. 원시 ID rc3-signed-missing-epoch_matches. 담당 Exchange, 의미 확정 CTO. 재시험은 누락/정상/모순 epoch와 snapshot_id 전체 출력 교차 비교.

계약은 snapshot 누락 시 ID null이라고 명시하지만 일부 필드 누락의 ID 보존을 별도로 확정하지 않는다. 따라서 새 보안 High나 이미 확정된 규약 위반으로 분류하지 않는다. 공통 출력 차이는 남아 있어 전체 일치 게이트 FAIL, 시험한 보안 거절 불변식 PASS로 구분한다. 임의 기대값 변경이나 구현 수정은 하지 않았다.

Linux CI에서도 동일 783/782/1을 재현했다. 이전 rc2 420/413/7 및 rc3 759/754/5 FAIL은 보존한다. 실제 ACK/WAL/원장/체인/REST/WS 및 제품 T01~T16은 NOT_RUN/NOT_CONNECTED. 검토 완료는 제품 PASS·출시 승인과 다르다.

## 고정 입력과 범위

CTO 원본 manifest SHA256 `e24c64d6ccb41f9fc22d6669ea57acd7036d2f1758a4127cba92ce49524823b5`를 다운로드해 확인하고 모든 commit/path tree를 대조했다.

|입력|SHA|
|---|---|
|A|549ce150d6a9f21ec30f159d39a4d91c31dbd759|
|C [NUS-12](/NUS/issues/NUS-12)|5d39b7a0ffe911ed60cd0e3a56ada135e76a425f|
|D [NUS-13](/NUS/issues/NUS-13)|7f9f14f35a985484cee64a5132ed26aa500c0887|
|E [NUS-14](/NUS/issues/NUS-14)|f9b5bbf1e06a4a0c43bebc6d5bd081dc3b1892c2|
|F [NUS-15](/NUS/issues/NUS-15)|99ee3bf73c9e44b6e7fef6d8640af14d587ea89b|

Contract `a71a8c03fea5e4d2876612e821eafcb4b359a0b132157929b9924d6f8fecd73e`, vectors `4851d9d674b2412ca8919d8347a71da13f9adf4426fe60b44e2a4a259f8bd948`. C/D/F 41 protocol 파일씩 A와 일치한다. E는 rc2 출처를 유지하며 schema/message-codec/s0-cases/batches 4개 실제 소비 파일만 rc3와 동일함을 확인했다. receipt/API 합성 회귀를 다시 실행했고 E 전체 rc3 구현이나 암호 구현으로 집계하지 않았다.

## 결과 해석

783 = 기존 759 + enum 정상 4조합×3언어 + epoch 4조합×3언어. 각 언어 261행이며 고유 취약점 수가 아니다. 기존 759개 ID·언어가 모두 보존됐음을 프로그램으로 대조했다. `finding-retest.json`은 기존 5개 실패의 before/after를 기록한다.

|범위|결과|
|---|---|
|기존 G-01~04 재현, 등록 키 타입/주소/도메인/만료 등호/정수/fee/cap|PASS|
|G-RC3-01 enum 3 부정 Rust/TS 4차이|수정 확인 PASS|
|G-RC3-02 stale epoch/true Rust 1차이|수정 확인 PASS|
|enum 1/2 네 정상 조합, epoch/flag 네 조합|24/24 PASS|
|실제 ML-DSA Go/Rust/TS producer×verifier×Order/Cancel/Wallet|27/27 PASS|
|생성 서명 codec, domain 변조, context 불일치|각 27/27 PASS|
|전체 비교|782/783, G-FIX-01 출력 차이 1개|
|정산 독립 합성 조건|20/20 PASS|
|실제 브라우저|392 conformance + 키 생성/서명/메모리 복구/390px PASS|

Go 공통 시험 264, Rust 17, 정산 16 및 TS test/build 통과. 브라우저 키는 임시 모의 키이며 외부 요청 0, pageErrors 0. 실제 REST/WS·체인 송신 증거가 아니다. 메모리 복구를 영속 백업/복구로 승격하지 않는다.

## 재현·조치 담당

새 checkout에서 `python3 security/prepare.py` 후 `bash security/run.sh`. 원시 입력은 `security/evidence/repro-missing-epoch.json`; 빌드 뒤 각 runner에 이 JSON 한 줄을 전달한다. 기대는 snapshot_id=synthetic-1, Rust 실제 null이며 나머지 결정 상태는 동일하다. `summary.json`에 전체 기대/실제가 있다. `protocol/v1/DECISION-PORT.md` 출력 정의와 `exchange/src/decision.rs::admit_order`의 null 치환을 대조했다.

G-FIX-01 담당 Exchange [NUS-13](/NUS/issues/NUS-13), 의미 결정 CTO [NUS-10](/NUS/issues/NUS-10). 심각도 Low, 보안 거절은 유지된다. CTO가 일부 필드 미연결 시 ID의 의미를 명시하고 필요하면 원래 구현/공통 벡터로 반환한다. 현재 기대를 승인된 새 규약으로 취급하지 않는다. 검증은 정상·누락·모순 snapshot/epoch 입력의 전체 출력과 기존 783행 회귀다. 이 보고서는 구현 수정이나 원래 업무의 재개를 수행하지 않았다.

## 재사용·미연결 경계

기존 고정 lock과 실제 설치 원본 LICENSE hash를 재확인했다: CIRCL 1.6.3(BSD-3-Clause), fips204 0.4.6(MIT/Apache-2.0), noble post-quantum 0.4.1 및 hashes 1.8.0, scure/base 1.2.6(MIT), OrderBook-rs 0.13.1(MIT). `licenses-and-locks.json`은 이전 검토와 동일한 hash임을 확인한 증거다. 새로운 라이선스 해석이나 버전 업그레이드는 없다. 기존 출처와 제한은 REVIEW.md/RETEST-RC3.md에 보존한다. 독자 암호 설계는 도입하지 않았다.

실제 자산 보존·음수 잔고/중복 지급·출금/정산 경합·후원 우회·직접 회수·영속 ACK/WAL 재생은 NOT_RUN. 원장·체인·DB·REST/WS는 NOT_CONNECTED. 잠정액 비가용/과거 receipt/동일 bytes 재시도/timeout 보류는 합성 adapter 범위만 PASS다.

Security 작성 harness의 독립 검토는 CEO 네이티브 review 및 [NUS-17](/NUS/issues/NUS-17)의 fresh checkout 재현으로 받는다. [NUS-11](/NUS/issues/NUS-11)을 G의 새 blocker로 추가하지 않았다. CEO 인수는 검토 산출물 완료이며 제품 PASS나 main 병합·후속 기능·출시 승인이 아니다.


## 최종 CI 증거

[PR #11](https://github.com/nus-gang/cosmo-dex/pull/11), [Linux CI 36620674595](https://github.com/nus-gang/cosmo-dex/actions/runs/36620674595), 시험 SHA `37c45de5a687eea9c33875d98d553971ea47168f`.

새 Linux checkout의 고정 입력·선행 시험은 통과했다. differential은 같은 출력 차이 1개로 failure이며 이를 숨기지 않았다. CI artifact를 내려받아 전체 783행 및 summary의 동일성을 확인했다. 실제 브라우저 단계는 success: 로컬 Chrome 154.0.8037.58과 Linux Chromium 140.0.7339.186 모두 392 conformance 동일, pageErrors 0·외부 요청 0·overflow false. 원시 workflow.log와 CI 증거는 ZIP의 security/ci-download-fixed에 포함한다.

최초 로컬 시도는 기본 Rust toolchain 미지정으로 중단됐고 설치된 Rust 1.92.0을 명시한 뒤 전체 시험을 실행했다. 이는 제품 결함 수에 포함하지 않았다. 마지막 증거 commit은 시험 소스를 변경하지 않는다. 새 Low 차이의 규약 정렬/수정 후 재시험은 NOT_RUN이다.
