# S2 결정·S1 회고·작업량

2026-10-02 CTO 작성. 실제 main/CI를 조회했으며 원격 main은 `24029b811e5ec798bbe57f769de3d3f254c90ab7`, 기존 9 jobs success였다. 이번 작업은 S1 제품 시험의 재실행이 아니다. 제품 인수 SHA `32781aa97d62ec747e7a25c10fdb8b58030d79f2`, 문서 main `24029b8`를 분리하여 기록한다.

## 짧은 S1 회고

| 발견·근거 | S2에서 고정한 대응 | 소유자·검증 |
|---|---|---|
| S1 network/account 높이·block age가 있어도 과거 COMMITTED를 현재 잔고로 읽을 수 있음 (`docs/api.md`, `settlement/s1/server.py`, Wallet 회귀) | 단일 ChainSnapshot, 연속 cursor, 5초 age, 미래 시계/역행/단절 닫기; account_generation과 seq 역행 폐기 | B/D/E, H/J AT05/07 |
| 화면 100 DEVQUOTE와 CLI 100000000 atoms의 단위 혼동 (`docs/api.md`, Docs 최종 100→40→60) | 6자리 정확 변환, lot1000/tick, C/R/D/P/A마다 denom·atoms/표시 단위; round 금지 | A/E/Docs, AT04/07/08 |
| 후보 PR 검증과 실제 main 검증이 달랐고 공통 snapshot/누락 의미의 수정이 여러 구현 재시험을 유발 (`protocol/v1/adr/G-FIX-01.md`) | A hash·schema·전체 출력 먼저 고정; F 통합 후보 하나; I main 반영 후 J 새 checkout | A/F/I/J, AT09 |
| reviewer가 통합 구현까지 떠맡을 위험, Docs 안내 재현도 별도 검증 필요 | 네이티브 approve/request_changes, 원 담당 수정, CEO I 병합, J 독립 main 재시험 | 모든 담당 |
| S1 실제 C/receipt는 존재하지만 Exchange ACK/WAL/예약은 미연결 (`exchange/README.md`) | S1 재사용과 S2 신규 기능을 구분; 실제 예치 후 주문 접수·재시작 증거 필요 | B/C/F/H |

M0 Exchange 결과/CTO 재심사에서 IOC Err+fill callback과 헤더 length 손상이 이미 발견됐다. 두 로컬 파일 fsync는 원격 복제 증거가 아니다. 완료 frame 유실을 로컬 정보만으로 항상 검출할 수 없다는 음성 시험도 보존한다. S2는 명시적인 local durability와 외부 ACK ledger, writer lock을 추가한다.

## 새 ADR

- **S2-01 프로필/서명**: 기존 S0 서명 wire를 그대로 유지하고 S2 chain/genesis/config에 새로 서명한다. 0bps 활성·25bps 별도 fixture, 기존 lot/tick/q/p 상한 유지. Order expiry 여유 2..1000은 개발 정책이며 S3 정산 SLA가 아니다. 담당 CTO, 근거 Chain/Exchange/Wallet, 목표 A 승인 전.
- **S2-02 snapshot/local durability**: 신뢰 RPC의 같은 높이 전체 snapshot과 연속 cursor, local fsync+marker+OS lock, 성공 영수증은 LOCAL_ACCEPTED. full state hash와 outbox를 원자 경계에 둔다. hash를 light-client 증명이라 부르지 않는다. 담당 CTO, 구현 B/C/D/F, 목표 착수 전 계약 승인.
- **S2-03 epoch 정정**: S2는 정산 미제출이므로 epoch 변경 owner가 속한 pending-fill 연결 성분 전체를 보수적으로 정정하고 관련 잔량을 종료한다. 기존 순서를 rematch하여 이미 성공한 fill ID/결과를 조용히 바꾸는 방식은 채택하지 않는다. 상대방의 독립 주문도 연결 성분이면 종료될 수 있으나 결과를 명시하고 새 서명 주문으로만 재개한다. P 재사용 금지·최악 D 유지·기록 보존을 지킨다. S3 in-flight settlement에는 재사용 불가. 담당 CTO/Exchange, Security/QA 필수 심사.
- **S2-04 로컬 인증**: S0 HTTPS default를 보존하고 S2 profile에만 loopback HTTP 5173 두 origin을 열어 기존 소형 UI를 확장한다. 실제 WalletChallenge와 세션·개인조회 권한을 추가한다. bearer token은 짧은 메모리 세션, 서버 키 수집 없음. 담당 CTO/Settlement/Wallet/Security.

## DEC-01~12 상태·주인·목표

모든 기술 합의 owner는 CTO, 사업/실운영 결정은 CEO가 별도 승인한다. 이번 S2 결정은 로컬 테스트에 한정하며 S0/S1 ADR을 삭제/소급 변경하지 않는다.

| DEC | S2 결정/범위 | 근거 담당 | 목표 시점/남은 게이트 |
|---|---|---|---|
| 01 자산 성격 | DEVBASE/DEVQUOTE 합성 genesis, 외부 상환 없음 | Chain/CEO | A 승인; 실제 genesis B/F, 실자산은 별도 승인 |
| 02 송금 fee | S0 계약 보존, 송금 경로 비활성 | Chain/Settlement | S2 착수 전 비활성 검증; 송금 활성 후속 승인 |
| 03 요청 ID | S0/S1 ID 보존, S2 order/nonce 영구 binding | Exchange/Settlement | A 승인 전 고정, C/D 재시도 시험 |
| 04 거래 fee | RECEIVE_ASSET_V1, 활성 0bps, 25bps 부정/ceil/cap | Exchange/Chain | A 승인, B/C 동일 profile |
| 05 만료/확정 | exclusive h<expiry, 신규 여유2..1000, local ACK·PENDING 구분 | Chain/Exchange/Settlement | A 승인, S3 batch 대기값은 후속 |
| 06 검증인/gas | 단일 호스트 4개, 기존 S1 gas fixture/실측 재사용 | SRE/Chain | F genesis/config hash 고정 시, 실운영 미정 |
| 07 키/서명 | ML-DSA-65 사용자, Ed25519 합의/P2P; 기존 wire | Chain/Wallet | A fixture, H 실제 교차검증; 키 영속복구 제외 |
| 08 직접 회수 | 사용자 직접 TX 유지, 별도 비상 RPC/sponsor 미제공 | Chain/SRE/Security | B/D/F 직접 TX 재현, T13 전체 후속 |
| 09 재사용/license | main lock 유지, OrderBook-rs0.13.1/fips2040.4.6/S1 SDK pin 보존 | Exchange/Chain/Wallet | 버전 변경 전에 CTO 심사, 새 도구 없음 |
| 10 정수/lot | U128/U256/U64, 6자리/1000/1 및 상한 고정 | 전체 | A 승인 전, C/E 동일 fixture |
| 11 정보 접근 | 공개 집계 호가/개인 인증 분리, 키 전송0, WAL GC없음 | Settlement/Wallet/Security | D/E/H 시험; 운영 개인정보 정책 별도 |
| 12 성능/복구 | 로컬 실제 관측만, TPS/RPO/RTO 미확약 | SRE/QA | 첫 영속 실제 서명 주문 뒤 재추정; 10-09 검토 |

버전/지원의 근거는 main에 보존된 `exchange/Cargo.lock`, `exchange/evidence/`, `chain/app/go.mod/go.sum`, `protocol/s1/evidence/` 및 공식 고정 태그다. 이번에는 버전 변경/새 라이선스 판단이 없으며 기존 증거를 재사용한다. OrderBook-rs runtime 채택은 C가 실제 adapter·lock·license를 확인해 제출해야 한다.

## 담당별 CTO 최초 추정

집중 작업일 범위이며 담당자가 가용량/납기를 수락한 값이 아니다. 실행 측정 초를 개발 일수로 환산하지 않는다. 신규 인력·유료 인프라 없이 기존 팀/로컬 호스트를 가정한다. 구현/검토 대기·CI 시간·실제 비용은 각 업무 manifest에 별도 기록한다. 승인하지 않은 비용/완료일 확약 없음.

| 업무/담당 | 입력→산출 | CTO 추정 집중일 | 의존·재추정 조건 |
|---|---|---|---|
| A CTO | S1/M0→이 계약/fixture·Security→QA | 1–2 | 현재 산출, 리뷰 대기는 미측정 |
| B Chain | A→두 자산·동일 H snapshot/보존식 | 2–4 | A done, 실제 예치 증거 후 재추정 |
| C Exchange | A→서명·원장·adapter·WAL·정정 | 5–8 | A done, 첫 LOCAL_ACCEPTED+재시작 후 재추정 |
| D Settlement | B/C→인증/REST/cursor/출금 준비 | 3–5 | 승인된 B/C 계약/head |
| E Wallet | B/C/D→서명 UI·단위·stale/정정 | 3–5 | 실 API 및 합성 schema 선행; 독립 구현 임의 착수 없음 |
| F SRE | B/C/D/E→새 genesis/통합 후보/CI/crash | 3–5 | 각 후보 head·CI, 같은 후보 G/H 사용 |
| G Docs | F→두 자산·주문·복구 사용자 안내 PR | 2–3 | CTO→QA 문서 검토, J가 안내 단독 재현 |
| H Security | F→독립 서명·권한·자산/복구 부정시험 보고 | 2–3 | CTO 검토, 부정 보고 done≠제품 PASS |
| I CEO | G 승인/H 실제 PASS/수정완료→보호 main 통합 | 1–2 | 보호 규칙·CI 우회 금지 |
| J QA | I main→새 checkout AT01~09·사용자 문서 검증 | 2–3 | exact main CI·H/G 제한 대조 |

합계 24–40 집중일(동시 작업 중복 포함), 개발 가용량·슬롯·리뷰 대기는 미확인. B/C 병렬 후 D/E/F 통합이 주 경로, 수정·통합·main QA·Docs에 가용량 약25–30%를 확보한다. 이 합계는 달력 24–40일이나 이번 주 완료 약속이 아니다. 첫 주간 검토 2026-10-09 Europe/London 17:00 (=16:00 UTC); 미완료 범위를 같은 의존 그래프의 다음 주기로 넘긴다. 장기 M1~M6 전체 구현·비용을 이번 S2 값으로 추정하지 않는다.

routine 소유/갱신은 부모 CEO 책임이며 A에서 타 업무를 변경하지 않는다. 본 문서는 예약 실행 결과가 아니다.

## 원본 확인

개발 설계서 원본 SHA256 `587f3782a531256eac6545884e7f91377120db9d74fab130e931c5605573f0b4`, 아키텍처 원본 `0e8e8ddd0484953339a2dee9964327436e79f018527870c69ba88e85315eb663`. 이번에 attachment를 재취득하여 해시 일치를 확인하고 같은 PDF의 보존 추출문에서 설계 6–10/14–15쪽의 보존식·ACK·정정·시험·DEC를 대조했다. 출처: [개발 설계서](/api/attachments/9c378275-dcec-4402-8478-4bf7d26af911/content), [아키텍처](/api/attachments/d157936f-f4b3-45e4-849a-f68621a7b90d/content), [M0 결과](/NUS/issues/NUS-4#document-m0-results), [CTO 재심사](/NUS/issues/NUS-4#document-cto-review), [문서 인수](/NUS/issues/NUS-34#document-cto-doc-final-24029b8).
