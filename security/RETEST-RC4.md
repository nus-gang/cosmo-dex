# NUS-16 rc4 독립 전체 재시험

2026-09-29 · Security. 로컬 및 Linux CI 공통 출력·시험 범위 보안 불변식·실제 ML-DSA 상호운용 PASS. CEO 독립 산출물 검토와 QA 인수는 별도다.

## 입력과 범위

A 57c187f5474e02c3f624667d3b8380268e13dd1a, C b0283833a0d040366219b35de58a3150d3956e92, D 7c7d0ca68b98f6c82fd297fa0dcd0a649194139d, E f9b5bbf1e06a4a0c43bebc6d5bd081dc3b1892c2, F 5ff848415c3fda4e2285634950848c99824193be.

contract afa3471a2210a90cde0004b3219b231468bf0b8471e8068f8e16b096b3549d84; vectors bb1b437d23a365f1083e85353bf62bcef6517cda94288ff7e2d0b4cb5fccd55b; snapshot-output 6508028f349af5aa127e148a808e075389eb450e81a8976ff32bad8cd065c884.

API에서 A/C/D/F done/approved를 확인하고 각 승인 제출 SHA를 읽었다. C/D/F 각각 44개 protocol 파일은 A와 byte-identical. E rc2 출처를 유지하고 실제 소비 schema/message-codec/s0-cases/batches 네 파일의 동일성을 검사했다. E의 결과는 합성 receipt/API 검증이며 암호 구현으로 집계하지 않는다. 구현 소스 변경 없음. 별도 Security checkout과 test adapter만 수정했다.

## 실행 결과

- 총 969 비교, 969 일치, 0 차이. 기존 783개 ID/언어를 모두 보존하고 rc4 60×3=180 비교 및 독립 null/누락 epoch+false 2×3=6 비교를 추가했다.
- 기존 binding/epoch 불변식 36행은 rc4 규범의 전체 출력 기대값으로 강화했다. 과거 기대/FAIL 증거는 evidence-fixed 및 이전 PR/첨부에 보존한다.
- 새 60건은 인증 결과를 주입하지 않는다. 공개 모의 seed로 OrderV1 projection을 재직렬화·재서명하고 각 언어의 실제 인증/정책 함수를 호출한다. 인증 거절은 서명 변조, 인증 NOT_CONNECTED는 등록 키 타입 누락으로 재현한다. 원본 context 누락/null은 Go Context JSON presence, Rust observation, TS optional 필드에 전달한다.
- 실제 ML-DSA Go/Rust/TS 생성×검증 27, codec 27, domain 거절 27, context 거절 27 PASS. wire·정수·overflow·만료 등호·등록 키 타입/bytes와 주소 결합·fee/cap 회귀 PASS.
- 로컬 실제 Chrome 154.0.8037.58: conformance 453 PASS, 키 생성/주문 서명/메모리 복구/390px UI PASS, pageErrors 0, 외부 요청 0. 영속 백업은 미구현이다.
- Go 338, Rust 18, TS 구성요소 시험·build, 정산 16 및 독립 합성 adapter 20 검사 PASS. 정산 과거 receipt·동일 bytes 재시도·timeout UNKNOWN·잠정 수취액 비가용의 모의 계약 경계만 검증한다.

## findings 재시험 및 잔여 위험

| 항목 | 심각도·재현 조건 | 결과·담당·검증 |
|---|---|---|
| G-FIX-01 | Low, epoch_matches 누락 시 원본 snapshot ID 손실 | rc4 규범 확정 및 C/D/F 정렬 후 해당 기존 3행과 신규 전체/부분/null/모순 snapshot 180행 PASS. CTO/Exchange가 소유한 원래 수정의 독립 재시험 완료 |
| G-RC3-01/02 및 G-01~04 | 역사 FAIL, enum/epoch/key/fee/판정 경계 | 기존 ID 회귀 PASS. 기존 420/413/7, 759/754/5, 783/782/1 FAIL은 과거 입력 결과로 유지 |
| 신뢰 context 및 상태 원자 소비 | 통합 조건부 High: 사용자 공급 snapshot을 신뢰하거나 합성 PASS를 ACK로 연결 | Chain/Exchange/Settlement 담당. 실제 앱에서 권한·잔고·중복·경합·재생·확정 관측 검증 필요. 현재 NOT_CONNECTED/NOT_RUN |
| 직접 회수·후원 우회·영속 백업 | 제품 연결 전 미검증, 위험도는 실제 경로에서 재평가 | Chain/Wallet 담당. 실자산 사용 없이 후속 승인된 단계에서 시험. 현재 NOT_RUN |

고정 lock 및 설치 원본 LICENSE hash가 이전 검토와 동일함을 재확인했다. CIRCL 1.6.3 BSD-3-Clause; fips204 0.4.6 MIT/Apache-2.0; noble/scure MIT; orderbook-rs 0.13.1 MIT. 독자 암호를 설계하지 않았으며 라이브러리 시험을 FIPS 인증으로 주장하지 않는다.

## 재현 및 인수

새 checkout: `python3 security/prepare.py` → `bash security/run.sh`. macOS 실행에서는 설치된 Rust 1.92.0의 CARGO/RUSTC/RUSTDOC 절대 경로를 지정했다. 최초 기본 toolchain 선택 실패는 환경 문제로 구분한다. 실제 브라우저는 web에서 `EVIDENCE_DIR=../security/evidence/browser npm run test:browser`.

ACK·ledger·REST/WS·체인 NOT_CONNECTED; WAL·실제 자산 보존·출금/정산 경합·직접 회수·제품 T01~T16 NOT_RUN. 이 PASS는 승인된 S0 공통 규약 검증 범위이며 S0-H 최종 인수, main 병합·후속 기능·출시 승인이 아니다. Security harness의 독립 검토는 CEO 네이티브 검토 및 [NUS-17](/NUS/issues/NUS-17) 새 checkout 재현으로 받는다.

## 최종 CI 증거

[PR #12](https://github.com/nus-gang/cosmo-dex/pull/12), 시험 SHA `041fa5954f3fa950b100c7a505217d949165347b`, [Linux CI 36635390972](https://github.com/nus-gang/cosmo-dex/actions/runs/36635390972) success. 새 checkout에서 입력 검증·구성요소 시험·전체 differential·실제 Chromium 단계가 모두 성공했다. 내려받은 전체 969행은 로컬과 JSON 동일하며, 453개 브라우저 check ID도 동일하다. Linux Chromium 140.0.7339.186, pageErrors 0, 외부 요청 0, overflow false. 원시 workflow.log와 artifact는 증거 ZIP에 포함한다.

초기 963비교 SHA 3458d087의 CI도 success였으나 최종 판정은 null/누락 epoch+false 6비교를 포함한 041fa595 기준이다. 마지막 증거 commit은 시험 소스를 변경하지 않는다. C/D/E/F 승인 상태·manifest·라이선스/lock·기존 ID 보존 비교는 evidence에 기록했다. 새 미해결 S0 차단 finding은 없다. 최종 S0 게이트는 [NUS-17](/NUS/issues/NUS-17)의 별도 fresh checkout/공통 CI 인수이며 이번 검토의 PASS와 구분한다.
