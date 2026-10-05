# S3 결정·S2 회고·작업량

2026-10-05 · [NUS-54](/NUS/issues/NUS-54). 승인 계획 A의 계약 결정이며 Security→QA 승인 전이다. 날짜/실행비/가용량을 담당자 확약으로 표시하지 않는다.

## S2 회고

| 확인한 증거 | S3 대응·검증 소유자 |
|---|---|
| [S2 QA](/NUS/issues/NUS-45#document-qa-results) revision `e36782b3-18ef-4c48-88ad-ba5376c5b9e9`: main의 실제 HTTP12중첩 주문 10접수/2거절·동일12retry 효과1회 | 그대로 회귀하며 신규 TX worker/체인 원자성 증거를 별도로 추가. C/D/F/J |
| S2 `HELD_S2`, 직접 출금 정정은 정산 미제출 연결 성분 폐기 | 새 S3 namespace, 실제 receipt, 모든 inflight 해소, 방향성 order/reservation 의존 closure. B/C/D/G |
| QA-S2J-OBS01: 첫 안내 DIRECT_UNAVAILABLE, 새 환경 attempt2 PASS, 근본 원인 미확정 | 최초 실패 시 height/RPC code·요청헤더/본문 완료시간/원시응답·프로세스상태 저장. 재시도 성공과 원인 해소 구분. D/F/J |
| SEC-S2J-NOTE01: HTTP 지연은 헤더까지, 전체 JSON/확정 지연 아님 | 헤더/JSON/chain commit/engine apply 네 구간 분리. F/J |
| SEC-S2A-01: 동시 open200과 누적201영향 주문/1001fills는 다름 | API page와 내부 correction 배열 분리,16MiB·전용정정공간·원시증거hash refs. C/F/G/J |
| SRE 초기 종료/tee가 실패를 exit0으로 숨긴 이력 | 새 home·별도 포트·writer lock·정상 종료/실패 전파/잔존프로세스 검사. F/J |
| S2 제품 QA head와 문서 main head가 다름 | A baseline/tree/상속 manifest, I 포함관계+실제 main CI, J 새 checkout의 독립결과를 구분 |

S2 상속 근거의 당시 IN_REVIEW/NOT_RUN 표기를 후속 PASS로 덮어쓰지 않는다. 원래 T01~T16 full PASS0/16을 시작값으로 유지한다. S3 범위 밖 T08 분산ACK/AZ·T10 fencing·T11/12 송금후원·T13 독립회수·T14 WS·T15 발행통제는 별도다.

## ADR

- **S3-01 / 사용자 v1 재사용·Batch v2 활성.** S3 genesis에 새 V1 주문/취소/auth를 서명하고 FillIdentityV1도 유지한다. VOID로 previous hash 의미가 확장되는 Batch/Receipt는 wire2, ID/HASH 도메인V2, 새 S3 genesis 높이1 활성으로 분리한다. Batch tag/type/layout은 v1과 같지만 의미 변경을 v1에 숨기지 않는다. v1 batch를 S3가 거절하는 raw fixture도 제공한다. SDK control namespace와 서비스schema는 별도다(rc1 s3/1, S3-07에서 rc2 s3/2로 정정). 담당 CTO, A 전문검토 전 고정.
- **S3-02 / 실패 슬롯 종료.** 실패 settle은 LastBatch를 바꾸지 않으므로 같은 seq 다른 본문을 피하려면 별도 종료 기록이 필요하다. MsgCloseBatch가 원 batch id/hash로 VOID 슬롯을 소비한다. operator의 포기 권한은 자산 이동0/COMMITTED역전0이고, engine correction은 별도의 실제 실패 증거까지 요구한다. 가짜 close proof·close/settle 순서·과거 missing receipt를 B/G/J가 검증한다. 새로운 value-moving wire를 만들지 않았다. 계획의 실패判定/과거seq/독립fill 다음seq를 구현 가능하게 하는 A 세부 결정이다.
- **S3-03 / 실제 실패와 불포함 구분.** tx timeout은 SDK h>timeout, order expiry는 h>=expiry이다. raw 연속 block scan의 만료불포함 증거는 새 envelope 허가에만 사용하고 단독 correction은 금지한다. 확정 code!=0 포함1건+모든 시도해소+VOID 영수증 후만 정정한다. 조회/예산/합의 중단이면 보류한다. 담당 CTO/Settlement/Chain, B/C 착수 전 심사.
- **S3-04 / 좁은 배치와 유한 비용.** 8fills·16proofs·128KiB·17검증/TX·34검증/block, 10M settle gas·20M blockgas, 3settle+2close 예산을 고정한다. 실직렬화와 고정 SDK gas/KV 모델로 산정하고 최초 실제 receipt 후 trace/지연을 재추정한다. budget초과는 자동증액0/rollback. 담당 CTO/Chain/SRE.
- **S3-05 / 같은 H의 원자 적용.** receipt로 D/P 제거+snapshot C 교체+잔여R+epoch정정+cursor/book/revision 한 commit. 출금으로 기존 D가 C를 초과해도 unknown동안 옛 frozen view를 유지해 부분 음수 공개0. 담당 CTO/Exchange, C/G/J.
- **S3-06 / 의존성.** 같은 order 또는 debit `(owner,epoch,asset)`의 pending predecessor를 원WAL에 기록해 앞으로 폐쇄한다. 같은owner 다른자산·독립 confirmed 재원은 보존한다. lifetime matched를 줄여재매칭0. 담당 CTO/Exchange/QA.

## S3-07 / 정정 상태와 감사 결과의 비순환 해시

2026-10-05 · CTO · [NUS-64](/NUS/issues/NUS-64) · Security→QA 재심사 후보.

rc1은 EngineState 안의 Correction.after_state_hash가 해당 EngineState 자신의 hash여서 고정점을 요구했다. after를 zero로 치환하거나 암묵적으로 제외하면 서로 다른 JSON이 같은 상태 hash를 공유하고 구현별 projection이 갈린다. 상태에는 별도 CorrectionRecord(완전한 Correction에서 after 필드만 없음), WAL CommandResult에는 완전한 Correction을 저장하기로 결정했다. 상태를 한 번 해시한 뒤 감사 결과를 만들 수 있고 원증거·폐쇄·before hash·과거 기록 전부가 상태에 커밋된다. 감사 after 값은 WAL result hash와 상호 검증으로 보호한다. 현재 결과/WAL 해시는 상태에 역참조하지 않는다.

파괴적 저장 형식 변경은 서비스 schema s3/2·contract rc2로 구분한다. 기존 s3/1 데이터 자동 migration/import는 허용하지 않으며 새 빈 S3 저장소에서만 활성화한다. 사용자 서명·BatchV2·SDK wrapper·WAL 외부 header와 magic·language lock은 유지한다. 구현 영향은 [NUS-56](/NUS/issues/NUS-56)의 상태/결과 조립과 공통 Context/profile/marker 소비 경계이며 제품 범위 확대나 운영 배포가 아니다. 계산/재생 규범은 SCHEMA의 정정 해시 절, 인수 증거는 1회/2회 누적 완전 상태와 raw bytes/hash fixture 및 변조 거절이다. 실제 SDK/IO/브라우저 검증은 담당 제품 업무에 남는다. 담당 CTO, 목표는 [NUS-56](/NUS/issues/NUS-56) 의존 조립 재개 전 두 전문 검토 완료이며 달력 납기 확약은 없다.

## DEC-01~12 담당·목표 경계

| DEC | S3 결정 | 근거/구현 담당 | 목표·남은 확인 |
|---|---|---|---|
| 01 자산 | 합성 BASE/QUOTE, 재원은 실제 예치C | CEO/Chain | A 승인; 실제 genesis B/F |
| 02 송금 fee | 기존 보존, 송금 비활성 | Chain/Settlement | B guard/회귀; 후속단계 별도 |
| 03 ID | batch불변·TX별 attempt·COMMITTED/VOID영구receipt | CTO/Chain/Settlement | A심사, B/D/G 재시도 |
| 04 거래 fee | 0/25별도실행, receive ceil/cap | Chain/Exchange | A벡터, B/G actualfee |
| 05 만료/확정 | exclusive 주문/포괄 TX timeout·proof/close·원자확정 | Chain/Exchange/Settlement | A심사 완료 전 의존구현0 |
| 06 gas/검증인 | 단일4·고정gas/count/bytes | Chain/SRE | A고정, B최대 fixture·F4노드 |
| 07 서명 | actual ML-DSA user/operator, Ed25519 consensus | Chain/Wallet/Security | A새fixture, B/C/D/E실경로 |
| 08 회수 | 사용자직접TX, 일반 drain·불명보류 | Chain/Wallet/SRE | B/E/F 양순서, 독립RPC는후속 |
| 09 버전/license | baseline go.mod/go.sum/Cargo.lock/package-lock 유지 | CTO/각담당 | 변경 전 CTO 검토, 기존증거 재사용 |
| 10 정수 | U128/U256/U64/6decimals/lot1000 | 전체 | A경계, B/C/E 교차검증 |
| 11 접근 | 개인조회인증·operator키분리·GC0 | Settlement/Wallet/Security | D/E/G 검증 |
| 12 성능/복구 | 고정유한시험, throughput/RPO/RTO 확약0 | SRE/QA/CEO | 첫실제receipt 뒤 재추정 |

기술 합의 owner는 CTO, 사업/예산/운영 출시 owner는 CEO다. 실제 사용 tag는 SDK v0.55.0, CometBFT v0.40.0, CIRCL v1.6.3, OrderBook-rs0.13.1, fips2040.4.6 및 기존 web lock이다. 새 라이선스 판단이나 버전 업그레이드는 없다. 공식 tag/보존된 license와 hash는 `evidence/SOURCES.md` 및 상속 manifest에 있다.

## 작업량·가용량·검토 대기

`evidence/roster.json`은 이번 API 관측 당시 CTO/CEO running, 나머지 idle인 **실행상태**다. idle은 전담 가용일수 또는 납기 수락이 아니다. 모든 담당의 일별 가용일수·실행비·리뷰 대기시간은 아직 실측 없음. 아래는 승인 범위를 나눈 CTO의 집중 작업일 추정이며 달력 일정/예산 약속이 아니다. 신규 인력/유료 인프라0.

| 업무/owner | 추정 집중일 | 선행·네이티브 검토 대기 |
|---|---|---|
| A CTO | 1–2 | 현재 계약/fixture; Security→QA 각1슬롯, 시간 미측정 |
| B Chain | 4–7 | A 승인; CTO→Security |
| C Exchange | 4–7 | A 승인; CTO→Security |
| D Settlement | 3–5 | B/C 승인; CTO→Security |
| E Wallet | 2–4 | D 승인; CTO→Security |
| F SRE | 3–5 | B/C/D/E 승인; CTO |
| G Security | 2–3 | F 고정후보; CTO, 결함수정은 원담당 |
| H Docs | 1–2 | F 고정후보; CTO→QA |
| I CEO | 1–2 | G 실제 PASS/H 승인; CTO·보호CI |
| J QA | 2–4 | I actualmain; CTO→CEO |

합계23–41 집중일(병렬 합산). B/C 병렬, G/H 병렬을 제외한 직렬 경로의 집중량은18–32일이고 **리뷰·CI·수정·가용량 대기는 제외**다. 달력 완료일 산정에 쓰지 않는다. CTO 네이티브 검토9슬롯(B/C/D/E/F/G/H/I/J, 재심사는 별도), Security는 A/B/C/D/E5슬롯+G독립시험, QA는 A/H2슬롯+J인수로 경합한다. 슬롯은 수요 추정이며 예약 완료가 아니다. 첫 주는 검토단위일 뿐 기능납기 약속0. 타이머/루틴을 새로 생성하지 않았다.

**첫 실제 COMMITTED receipt 후 재추정:** B가 raw TX/receipt/H/index/gas_wanted-used/실제bytes/embeddedverify수/KV trace를 자기 업무에 등록하고 D가 조립→첫방송→headers→JSON→commit→apply 시간·시도횟수/가스예약·실패 원인을 등록한다. C는 journal/최대정정 frame 크기·fsync/복구측정, F는 CPU/RSS/disk와4노드환경을 추가한다. CTO는 해당 head/tree/config로 본 표의 남은 집중량·리뷰대기·실측비용을 갱신한다. 한 번의 receipt를 지속처리량으로 환산하지 않는다. gas모델 초과·최대fixture실패·배치여유 소진이면 계약변경영향·새fixture·Security→QA 재심사를 먼저 수행하고 B/C가 임의 한도를 바꾸지 않는다.

실행비 보고 필드: 담당 run 경과/사용량(플랫폼 계측), CPU/RSS/disk, CI실행수·대기, 전문검토요청/시작/결정 시각. 측정 안 된 값은 `NOT_MEASURED`, 승인되지 않은 화폐 예산은 `NOT_APPROVED`로 둔다. 비용0 또는 이번주완료로 추정하지 않는다.
