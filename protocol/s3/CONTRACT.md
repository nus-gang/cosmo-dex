# S3 실행 계약 1.0.0-rc2

2026-10-05 · CTO · [NUS-54](/NUS/issues/NUS-54). **Security → QA 재심사 후보. 두 승인 전 rc2 의존 상태/결과 조립 금지.** 승인된 [S3 계획](/NUS/issues/NUS-53#document-plan) revision `ece8cc33-a2e6-41ec-9810-0e4095fb3016`의 A 산출물이다. 제품 구현·실제 정산·main 인수 완료를 뜻하지 않는다.

[NUS-64](/NUS/issues/NUS-64)는 승인 rc1 head `0915375cac360f83d62a70587a0e7cf9c89604a1`의 상태 해시 순환 참조를 정정한다. 두 재심사 전 rc2 의존 조립은 열리지 않는다.

## 1. 권위·버전·환경

원격 main `bd9e473196ac86fdedf655b2c93e6931f54faa83`, tree `31a0d1c65e9b71647cac6c4c45bf7e8d2dd9d7f3`에서 독립 checkout을 만들었다. S2 제품 QA는 `aad654bcf6760bc9af162b681a5996487ffc715e`의 상속 증거이며 뒤의 문서 병합을 새 제품 QA로 세지 않는다.

규범은 이 문서 → `SCHEMA.md`/`batch.proto`/`messages.proto`/`schema.json` → `profile.json`/`errors.json`/fixture 순이다. 충돌은 구현자가 임의 선택할 사안이 아니라 계약 결함이며 접수를 닫고 A 수정·재심사를 요구한다. 기존 v1 rc4, S1 DIRECT, S2 매칭·WAL 규칙은 여기서 명시한 S3 확장 외 그대로다. `manifest.json`이 모든 상속 파일·규범·도구·fixture·lock 해시를 고정한다.

- **사용자 OrderV1/CancelV1/WalletChallengeV1와 FillV1/FillIdentityV1의 tag/type/frame/hash를 그대로 재사용한다.** ML-DSA-65 pure, FIPS context empty, raw pk1952/signature3309, owner=SHA256(pk)[:20], nus lowercase Bech32를 유지한다. randomized 유효 서명도 허용한다. deterministic seed는 공개 fixture 생성 전용이다.
- **BatchV2/BatchReceiptV2는 wire=2**다. VOID를 포함한 terminal 슬롯의 previous hash 의미가 확장되므로 v1과 구분한다. tag/type/layout은 v1과 같고, batch ID/HASH 도메인은 `NUS/BATCH_ID/V2`/`NUS/BATCH_HASH/V2`로 분리한다. 사용자 서명과 fill ID 도메인은 V1이다. 새 S3 genesis의 높이1에서만 v2 활성; S3 settle/close는 v1 및 다른 버전을 UNSUPPORTED_VERSION으로 거절한다. 기존 S1/S2 데이터/클라이언트의 자동 전환0. 향후 변경도 새 wire·벡터·활성 경계·CTO/Security/QA 승인이 필요하다.
- S3 서비스 schema는 `s3/2`; 기존 `s3/1`과 S2 JSON을 자동 승격하지 않는다. 정정 저장 형식 변경이므로 새 빈 S3 journal/marker/snapshot과 rc2 context에서만 시작하며 기존 저장소를 재해시해 import하지 않는다. S3W1 외부 프레임·72B header는 유지하고 context/marker가 `s3/2`를 강제한다. 추가 SDK 메시지는 `nus.exchange.s3.v1` namespace와 `SIGN_MODE_DIRECT`를 사용하며 그 안에 strict BatchV2 bytes를 담는다. SDK 메시지 namespace의 v1과 서명된 배치 protocol_version=2는 별개다.
- 단일 호스트 4 Ed25519 검증인, 신뢰 로컬 RPC, ML-DSA 사용자·운영자 TX, `DEVBASE/DEVQUOTE` 한 시장, `LOCAL_FSYNC`, `replicated=false`. `profile.json`(0bps)과 `profile-fee25.json`(25bps)는 **서로 다른 새 genesis 실행**이다. manifest가 두 config hash를 따로 고정한다. 실행 중 fee/config 변경 없음.
- `nus-s3-dev-1`, `.runtime/s3/` 아래 별도 genesis/home/engine journal/worker journal, 새 사용자·operator·admin 키. S1/S2 home 또는 실제 genesis와 같거나 `HELD_S2` import가 요청되면 시작 거절. 테스트 공개 seed를 runtime에 넣지 않는다. 실제 genesis 원본 bytes의 SHA256을 서명·앱·API·manifest에 결합한다. `vectors/genesis-fixture.bin`은 체인 genesis가 아니다.
- 기본 시연은 2사용자. 독립 fill/최대 배치 검증에는 최대 16등록 사용자, 별도 현재/후임 operator와 genesis admin을 허용한다. 계정별 두 자산 bank=10^12 atoms, GAS=10^9 atoms, C=0/E=0. 실제 예치만 C를 만든다. 합의키·운영키·사용자키·admin키를 재사용하지 않는다. 기존 4개 gas 배분 주소를 자동 정산 권한자로 바꾸지 않는다.
- 실자산·공개 배포·유료 자원·새 채용·분산 ACK/fencing·SLA·영속 지갑 복구·송금/후원·독립 비상 RPC/가스·WS 전체는 제외한다.

## 2. 정수·회계·매칭

JSON 정수는 `0|[1-9][0-9]*` 문자열, atoms=U128, q/p/epoch/seq/height=U64, match_index/cap=U32, 중간값 U256 checked. 부호/소수/지수/공백/선행0/중복·미지 key/범위 초과를 거절한다. API atoms base64 금지, wire atoms 정확히16-byte BE. q/p 각각 1..10^6, lot=1000 BASE atoms, lot-tick=1 QUOTE atom, decimals=6, 주문 quote<=10^12. open 주문 owner100/전체200. v1 signed cap은 U32 전체가 유효하고 활성 bps는 0..10000이다.

`base=q*1000`, `quote=q*execution_ticks`. 양측 limit, side, 서로 다른 owner, maker/taker가 buyer/seller의 정확한 두 참조라는 점, maker 가격과 주문별 누계를 검증한다. 가격/FIFO의 외부 공정성 증명은 제공하지 않지만 정산 가격이 양측 서명을 넘을 수 없다. fill의 fee version은 활성 설정과 일치해야 한다. active_bps>signed cap은 FEE_CAP. fill별 `ceil(receive*bps/10000)`, bps=0이면0, 양수 fee>=receive면 FEE_GE_RECEIVE. 수취 자산에서만 차감하여 T에 더한다. 401+401 atoms@25bps의 split fee=4, 합산1회=3이므로 합산 반올림 금지.

Chain은 **배치 처리 직전의 C_start**를 복사하고 owner/asset별 모든 fill의 gross debit을 합산하여 `gross<=C_start`를 먼저 검사한다. 배치 중간의 수취/차액/netting을 재원으로 쓰지 않는다. 유효한 전체 계산 후 C/T, order binding/cumulative, SeenFill, LastBatch, COMMITTED receipt를 **한 message cache commit**으로 반영한다. 하나라도 실패하면 이 집합과 U·교환 bank 모두 이전 값이다. SDK ante의 DEVGAS fee·account sequence는 별도로 남을 수 있으며 교환 rollback에 포함하지 않는다. 중복 누계·잔고·epoch overflow도 전체 실패다.

Engine은 `A=C−R−D>=0`, P는 사용0이다. 매도 D=q*1000, 매수 D=q*buy_limit을 유지한다. 체결가격과 limit 차액은 COMMITTED 적용 또는 확정 실패+폐쇄+정정 원자 commit 전에는 풀지 않는다. 미체결 취소/IOC/만료는 R만 반환한다. S2 가격/FIFO·IOC Err+callback·STP·LOCAL_ACCEPTED·ID binding을 그대로 보존한다.

0bps 대표값(모두 atoms): A가 BASE10000000, B가 QUOTE100000000 예치. A SELL2000lots@10000, B BUY1000lots limit12000, maker 가격10000. 잔량 취소 뒤 A D_BASE1000000/P_QUOTE10000000, B D_QUOTE12000000/P_BASE1000000, R=0. 불명 중 변화0. 확정 후 A C=(9000000,10000000), B C=(1000000,90000000), D/P=0. B 가용 QUOTE가88000000→90000000으로 증가하는 2000000은 이 commit에서만 해제된다. 수취 출금 후 A C_QUOTE0/B C_BASE0. 25bps는 buyer BASE997500/seller QUOTE9975000, T_BASE2500/T_QUOTE25000이다.

자산별 `module_bank=sum(C)+T+U`, C/T/U 음수0. 잔차>0은 U에 원자 격리·경보하고 C를 늘리지 않으며 정상 본인 출금을 유지한다. 잔차<0이면 ASSET_DEFICIT, 신규 정산 및 해당 자산의 자동 출금 준비를 닫는다. 직접 TX도 보존식/실제 bank 부족을 우회하여 지급할 수 없다. 임의 mint/burn으로 메우지 않는다. 개인 bank·모듈·T·U를 포함한 총 공급과 GAS를 각각 대사한다. T/U sweep 기능은 S3에서 비활성이다.

## 3. 배치·TX·영수증 식별

BatchV2 strict wire는 singular presence(0 포함), tag 비감소, minimal varint, nested 동일 규칙을 지킨다. unknown/duplicate/missing/wrong wire는 거절, 수신 배열 재정렬 금지. BatchCore는 tag7만 제외한다. `batch_id=SHA256(frame(NUS/BATCH_ID/V2,core))`, `batch_hash=SHA256(frame(NUS/BATCH_HASH/V2,full))`. fill ID는 v1 `(chain_id,market_id,operator_epoch,command_seq,match_index)` frame 해시다. genesis는 주문/배치/저장 namespace에 반드시 별도 결합한다.

fill은 (command_seq,match_index) 엄격 증가, 중복 ID/tuple 거절. 증빙은 사용된 distinct order의 order_hash순이고 미사용/중복 증빙 금지. **S3에서는 모든 참조 주문의 원문·서명을 매 배치에 넣는다.** 기존 binding이 있어도 같음을 검사하고 새 효과 경로에서는 다시 검증한다. 증빙 생략 최적화는 이번 프로필에서 비활성이다. 체인에서 본 order ID는 같은 본문에 영구 결합한다. 실패한 message의 신규 binding도 롤백한다. Engine의 LOCAL_ACCEPTED binding/매칭 lifetime 누계는 정정 후에도 보존한다.

시장당 SEALED 이후 미해소 후보는 최대1개. 그 뒤 잠정 fills는 batch seq를 미리 할당하지 않은 영속 FIFO에 남는다. 가장 오래된 유효 pending prefix 중 한도까지 seal한다. 배치 bytes/id/hash/seq/prev/fill IDs는 바꾸지 않는다. 방송 후 분할·fill 교체·새 ID 재발급 금지. 처음부터 한도에 맞는 prefix로 나누고 첫 후보 해소 후 다음 후보를 seal한다.

체인 `(genesis,market,seq)`는 **성공 또는 VOID로 소비된 슬롯**이다. 초기 LastBatch=(0,zero32), 다음 seq=last+1, prev=last.hash. 슬롯 hash는 언제나 그 슬롯의 원래 canonical batch_hash다. 성공 receipt는 BatchReceiptV2이며 최초 성공 TX hash/높이를 유지한다. VOID는 별도 ResolutionReceipt이고 COMMITTED로 표시하지 않는다. 둘 다 영구 보관·GC0.

새 효과 처리 순서: 자원/정규성/버전/context → 현재 operator TX 권한 → 과거 슬롯 조회 → 신규 seq/prev/operator epoch → 사용자 등록키/서명/binding → epoch/revoke/expiry → 시장/fee/누계/gross → 원자 commit. 과거 seq의 id/hash가 같고 COMMITTED면 ALREADY_COMMITTED 성공(no-op), VOID면 BATCH_CLOSED 거절(no-op), 다르면 BATCH_CONFLICT. 과거 성공에는 **현재 사용자 만료나 과거 operator epoch를 재적용하지 않는다.** 현재 operator 권한 없는 mutation은 실패하되 공개 조회는 가능하다. seq<=LastBatch인데 receipt가 없거나 hash 불일치이면 RECEIPT_INCONSISTENCY, 신규 이동·D/P 해제0, 복구 필요다.

TX hash는 정확한 SDK TxRaw bytes의 SHA256이다(API lowercase hex, Comet uppercase는 decode 후 비교). account sequence/timeout/gas/signature를 바꾸면 새 attempt/TX hash지만 Batch bytes는 동일하다. worker는 단일 operator account·OS writer lock을 독점한다. operator key를 브라우저·REST 응답·로그·artifact에 넣지 않는다. `MsgSettleBatch`만 큰 TX 한도와 비영 timeout을 허용한다. S1/S2 guard를 전역 완화하지 않는다.

## 4. 만료·용량·가스·재시도 수치

`profile.json`의 숫자는 아래 근거로 고정한 개발 안전 한도다. 보장 지연/TPS/운영 비용을 뜻하지 않는다. 실제 사용이 한도보다 크면 B/F 인수 실패이며 자동 증액하거나 fixture를 줄여 성공으로 만들지 않는다.

| 항목 | 고정값·판정 | 근거 |
|---|---|---|
| fills/orders/batch | 8 / 16 / 131072 bytes, empty 금지 | fill당 distinct 주문2개, ML-DSA 원문·서명 전체 포함; `capacity.json` 실제 직렬화 |
| SDK settle/close TX | 139264 bytes | batch128KiB+DIRECT envelope8KiB; memo/extensions/tip/feegrant/다중메시지 금지 |
| user TX | 16384 bytes | 기존 입출금·사용자 제어 메시지 작은 경계 유지 |
| nesting | 4 | 상속 strict codec 한도 |
| 서명 검증 | settle 최대17, 블록 최대34 | 16사용자+1operator; 2개 최대 단위의 상한 |
| consensus block | bytes1048576 / evidence65536 / gas20000000 | 2×10M settle 예산과 header/4 Ed25519 commit·다른 사용자 TX 여유; 무제한 -1 금지 |
| settle gas/fee | 10000000 / 20000 DEVGAS atoms | 기존 앱 10M ceiling, ceil(gas/500) |
| close gas/fee | 3000000 / 6000 DEVGAS atoms | 최대 TX bytes 비용1.393M+1서명+작은 슬롯 기록·ante 예산, 사용자 서명 재검증/원장 이동0 |
| user/control small TX | gas500000 / fee1000 | 기존 S1/S2 직접 TX 기준 |
| 자동 시도 | settle3, close2, 각각 동일 raw TX 재방송 최대3 | 총 최대36M gas / 72000 DEVGAS 예약; 불명 시 미소비로 환불0 |
| RPC/wait | timeout2000ms, poll1000ms, batch wait1000ms | S2 조회 경계와 1 nominal block 조립 주기; 시간으로 final 판정0 |
| 동일 TX 재방송 | 지연0/1000/2000ms | 해당 시도의 3번 한도; 재시작 후 카운터 초기화0 |
| 신선도 | 마지막 성공 조회와 block age 각각<=5000ms, 미래<=1000ms | S2 경계 유지, 초과/갭에서 접수·새 제출 닫기 |
| 주문 접수 만료 | delta20..1000, 기본100 blocks | 8블록 attempt+4블록 관측 여유12에 조립/처리8 여유를 더한 정책 |
| 신규 seal/첫 제출 | min(expiry)-H>=12 | 최신 committed H에 대해 재검사; 오래된 FIFO를 조용히 새 서명으로 대체0 |
| TX timeout | H+8 (U64 overflow 거절) | SDK는 h>timeout에서 거절, h==timeout은 실행 가능 |

주문은 항상 실행 h<expiry; h==expiry 거절이다. 첫 제출시 timeout=H+8<최소 expiry를 보장한다. 대기 fill이 이 여유를 잃으면 신규 매칭을 닫고 기존 D/P를 보존한다. **이미 seal된 원 batch**의 재시도에는 같은 8블록 timeout을 새 봉투에 줄 수 있고, 주문 만료/epoch 변경이 확인된 경우 확정 실패를 얻기 위한 재시도도 예산 안에서 허용한다. 이 경우 사용자 주문 검사는 완화하지 않아 EXPIRED/EPOCH_MISMATCH로 실패한다. 단순 만료 관측만으로 seal된 fill을 정정하지 않는다. 미제출 큐도 최신 H에서 이미 만료됐거나 확정 revoke/epoch 변화로 영구 무효임이 확인되면 RESOLVE_FAILURE 목적을 WAL에 기록해 원본 prefix를 seal할 수 있다. 이는 12블록 여유의 유일한 예외이며 실제 주문 검사를 완화하지 않고 실패 증거만 얻는다. 단순 여유 부족·잔고 부족만으로 이 예외를 적용하지 않는다. 아직 유효하면 보류하여 안전한 여유 또는 영구 무효 증거를 기다린다. 재시도 예산 부족이면 RECOVERY_REQUIRED 보류이며 숨은 무한 시도/자동 budget reset은 없다.

SDK v0.55.0 공식 auth param은 bytes당10gas, ML-DSA 검증750gas다. Chain은 embedded 주문 검증마다 동일750을 명시적으로 charge하고 account/signature caching으로 검증 개수를 축소 보고하지 않는다. proposal 검증 전에 strict 구조에서 서명 수를 산정한다(settle=1+distinct proofs, 나머지 TX=1). PrepareProposal/ProcessProposal의 같은 count 규칙으로34 초과를 거절하고 FinalizeBlock에서 동등하게 확인한다. 악성 큰 배열은 crypto 전에 RESOURCE_LIMIT이다. 이는 합의 서명 검증 횟수가 아니라 앱 TX/주문 검증 예산이다.

고정 store/v2 가스 기준의 정산 상한 모델은 `139264*10 +17*750 +128*1000 +262144*3 +96*2000 +131072*30 +500000 = 6943982`이다. keeper 경로는 Has 포함 read128회/총 key+value256KiB, write96회/총128KiB 이내, unbounded iteration0을 요구한다. 마지막500k는 기타 ante/store 작업의 예산이다. **이는 실행 gas 실측이 아니다.** B는 최악 fixture의 실제 gas·KV trace가 이 bound와10M을 만족함을 입증한다. 초과는 KV_BUDGET_EXCEEDED/OutOfGas 전체 rollback이고 A 수치 변경은 재심사 대상이다. CIRCL 34검증 microbenchmark 원시 samples도 보존하지만 gas나 블록시간으로 환산하지 않는다.

## 5. 시도 해소와 확정 실패 증거

시도마다 서명 전 필요한 최신 account_number/sequence/H를 조회한다. `Batch bytes → Attempt(raw TxRaw/hash/seq/timeout/gas/fee/first_possible_height) → WAL fsync+marker → 방송` 순서를 강제한다. PREPARED를 포함해 방송 가능성이 있는 모든 시도를 조회 대상에 넣는다. first_possible_height=서명에 사용한 관측H+1이며 이후 바꾸지 않는다. seq를 로컬로 추측하여 다음 봉투를 만들지 않는다. 하나의 시도가 끝나기 전 새 envelope 서명0; 동일 raw TX 재전송만 가능하다.

권위는 신뢰 로컬 RPC의 확정 block+block_results와 동일 committed H의 module receipt/C/LastBatch다. `/tx`의 NOT_FOUND, CheckTx, mempool, HTTP 성공, 헤더만 받은 응답, timeout은 포함/실패/불포함의 증거가 아니다. 조회 불일치는 원문 보존 후 닫는다. 결과 검증은 raw block의 tx[index] hash=저장 TxRaw hash, 결과의 같은 index/code/gas, block height/hash/chain, receipt context/id/hash를 대조한다. snapshot H와 ABCI 응답H도 동일해야 한다. 이 신뢰 경계를 light-client proof라고 부르지 않는다.

| 시도 증거 | 판정·허용 |
|---|---|
| receipt COMMITTED 일치 + 최초 성공 TX 포함 증거 | 배치 COMMITTED. 다른 시도의 늦은 실패가 이를 뒤집지 않음 |
| code!=0 확정 포함, receipt 미적용, 동일 H LastBatch 대조 | 해당 attempt INCLUDED_FAILURE. 배치 전체 실패는 다른 시도까지 검사 필요 |
| code=0인데 대응 success/duplicate/VOID receipt 없음 | RECEIPT_INCONSISTENCY, 보류·정지 |
| timeout 뒤 NOT_FOUND만 | SUBMISSION_UNKNOWN, D/P/가스 예약 유지 |
| 확정 H>timeout, first_possible_height..timeout 모든 raw block+results의 연속 스캔, 해당 tx 불포함, 동일H receipt 미적용/LastBatch 일치 | EXPIRED_ABSENT_PROVEN. 이 raw TX의 미래 포함은 SDK timeout으로 불가능. 새 봉투는 가능하나 이것만으로 CORRECTED 불가 |
| scan 누락/pruned/gap/상이한 genesis·hash | 불명 유지. sequence 증가만으로 어떤 batch가 처리됐는지 추정0 |

한 시도의 최대 스캔은8블록이다. 인덱서 가용 여부와 무관하게 block 원문을 읽고 tx hash 및 결과를 확인한다. h==timeout 관측은 만료 증거가 아니다. account sequence 변화는 부가 대조 값이며 혼자 종료 근거가 아니다. 원래 TX가 확정 실패했더라도 다른 시도가 모호하면 배치 SUBMISSION_UNKNOWN이다.

**REJECTED_FINAL 진입 조건:** (a) 적어도1개 원 settle TX의 확정 code!=0 포함 증거, (b) 모든 나머지 시도가 INCLUDED_FAILURE 또는 EXPIRED_ABSENT_PROVEN, (c) 동일H에 COMMITTED receipt 없음/LastBatch 일치, (d) 증거 집합이 WAL에 저장됨. 성공 receipt가 있으면 언제나 COMMITTED가 우선한다. 기대된 EPOCH_MISMATCH/ORDER_REVOKED/EXPIRED/INSUFFICIENT_CONFIRMED_BALANCE 및 확정 회전 증거가 있는 OPERATOR_EPOCH_MISMATCH만 자동 정정 후보로 분류한다. 서명·binding·보존·prev/seq·내부 오류·OutOfGas 등 예상 밖의 실패는 UNEXPECTED_FINAL_REJECTION으로 닫고 원 담당 수정·전문 검토를 요구한다.

### 실패 슬롯 폐쇄 — ADR S3-02

실패한 settle TX는 LastBatch/receipt까지 롤백하므로 새 독립 fill을 같은 seq의 다른 본문으로 바꾸면 불변 binding이 깨진다. S3는 별도 **MsgCloseBatch**로 원래 후보를 VOID로 소비한다. 원 settle 실패 TX와 별도의 성공 TX이며 교환 자산·SeenFill·주문 누계·binding에는 효과0이다.

현재 operator가 원 batch bytes와 failed_tx_hash/proof-set hash를 DIRECT 서명한다. Chain은 자원/strict bytes/context/id/hash/current operator, next seq/prev 또는 기존 슬롯을 검사한다. old candidate의 operator epoch/만료/사용자 서명 정책은 다시 통과시킬 필요가 없다. 신규 close는 VOID receipt와 LastBatch=(원seq,원batch_hash)만 원자 저장한다. 이미 COMMITTED면 그 success receipt를 반환해 폐쇄하지 않는다. 이미 같은 VOID면 원 VOID receipt no-op, 다른 본문이면 BATCH_CONFLICT다. 이후 늦은 old settle은 BATCH_CLOSED로 실패한다. 다음 batch는 seq+1, previous=VOID된 원batch_hash다.

**Chain의 close 권한은 operator의 미확정 슬롯 포기 권한이지 실패 proof 검증을 가장하는 API가 아니다.** failed_tx_hash/evidence_hash는 감사 결합값이고 arbitrary operator가 거짓 값을 쓸 수 있다. 따라서 Engine/worker는 close receipt만으로 정정을 승인하지 않으며 위 (a)~(d)의 원시 실패 증거도 독립 검증한다. operator가 settlement를 검열할 수 있다는 기존 신뢰보다 넓은 자산 권한을 주지 않는다. COMMITTED를 VOID로 덮어쓰는 경로는 없다. 이 제약을 부정 fixture/실제 G 시험으로 확인한다.

close도2시도·결과 불명·방송 전 영속화 규칙을 따른다. close receipt 미확정이면 CLOSING으로 보류한다. 모든 실패 증거+VOID receipt가 같은 chain history에 확인돼야 CORRECTION_READY이며 아직 D/P를 해제하지 않는다. 예산 소진/chain 정지/증거 소실은 RECOVERY_REQUIRED; 새 batch 생성0/가격 개선 해제0. 자동 재시도3+2의 예약은 항상72000 atoms를 먼저 확보하며 직접 사용자 출금 GAS와 섞지 않는다. 성공 후 미방송 시도에 대한 예약만 해제한다. 소진 후 추가 시도는 CTO 검토된 새 budget manifest·명시적 재개에 한정하고 기존 카운터 이력은 유지한다.

## 6. COMMITTED·snapshot 원자 적용

S3 ChainSnapshot은 같은 확정 H의 C/owner epoch/key/sequence, bank/T/U/supply, operator epoch, LastBatch 및 H에 발생한 terminal slots·owner events를 반환한다. 연속 H cursor, query H, raw block H/time/hash, context/config/owner 집합을 확인한다. H header app_hash를 H 실행 후 root라고 잘못 표현하지 않는다. receipt는 원래 commit H_b<=H이고 H에서 영구 조회된다. (이전 cursor,lastSeq)에서 새 LastBatch까지 모든 슬롯의 원문 receipt/성공 TX 또는 VOID 증거가 필요하다. 누락되면 fail-closed다.

Engine은 공개 상태와 별도 비공개 candidate에서 다음을 **한 commit**으로 처리한다.

1. 성공 slots를 seq순으로 검증하여 해당 pending fills만 COMMITTED, 그 fill의 worst D/net P/잠정fee를 정확히 제거한다. 체인 C에 P를 더하지 않고 **snapshot C로 교체**한다. 이미 같은 batch_id/hash/revision이면 경제 효과0이다.
2. 같은 H의 owner event를 TX index순으로 대조한다. 정산→출금이 같은 블록이면 성공 fill을 먼저 보호한 뒤 미확정 fill만 새 epoch 정정 대상으로 삼는다. 출금→정산이면 failed TX와 폐쇄 증거를 모은다. H 이후 receipt를 H의 C와 섞지 않는다.
3. 필요한 실패 closure/영향 잔량 종료·정정을 완료하고 살아남은 주문 R과 pending D/P를 원문에서 재합산한다. C/R/D/P/A·T/U·order cumulative/SeenFill/LastBatch와 불변식을 검산한다.
4. 원 receipt/proof refs, chain cursor/snapshot id, book/FIFO, bindings, outbox, batch state, correction revision, stream seq, 전체 상태 hash를 한 WAL commit/marker에 저장한 뒤 snapshot과 응답을 공개한다.

직접 출금 관측으로 새 C가 기존 R+D보다 작아졌지만 in-flight가 불명인 경우, **이전 적용 C/R/D/P를 수정하지 않은 frozen view**와 별도의 latest_observed_height를 표시한다. 해당 옛 A를 출금/주문 가능액으로 노출하지 않는다(fresh=false, admission=false). 새 chain C만 끼워 넣어 음수 A를 만들거나 D를 먼저0으로 만들지 않는다. 필요한 증거가 모였을 때 위 원자 commit이 새 C와 전체 정정을 함께 공개한다. 같은 H 여러 TX 사이의 중간 C를 snapshot으로 가장하지 않는다.

## 7. 직접 출금·취소·operator epoch

일반 준비는 owner 접수/매칭 동결 → 미체결 잔량 취소(R만) → pending/불명 배치 판정·원자 적용 → D/P=0·미해소 attempt0·fresh 확인 → 사용자 DIRECT 서명 순서다. readiness는 권한 토큰/자동 출금 승인이 아니다. UI가 READY를 보여도 체인에서 재검사한다. 자동 서명/자동 재출금0. 명시적 abort도 준비 H보다 높은 fresh snapshot, 전체 mode OPEN, 불변식 확인 뒤 새 주문만 허용하고 취소 주문을 부활시키지 않는다.

직접 MsgWithdraw는 엔진 승인 없이 owner=signer=수취인, h<expiry, current epoch/잔고를 검사하고 C 차감·bank 본인 지급·owner 전체자산 epoch+1·receipt를 원자 commit한다. 실패는 epoch 불변(ante GAS/sequence 별도). withdraw의 idempotency는 S1/S2대로 원 receipt를 반환하며 2회 epoch 증가0이다.

`ledger.json/withdraw_order`의 양 순서를 barrier와 (height,tx_index)로 증명한다. B의100 QUOTE 전액 또는1 atom 출금이 먼저면 이전 epoch의 배치가 거절되어 A/B 수취0이다. 정산이 먼저면 B C_QUOTE90, 전액100 출금은 실패/epoch0; 새90 출금 또는1 atom은 성공/epoch1이다. 이미 확정된 fill은 유지된다. 같은 블록의 두 index순서도 각각 시험한다.

오프체인 CancelV1은 기존 pending D를 무효화하지 않는다. 온체인 `MsgRevokeOrder`는 자기 strict OrderV1의 hash/owner/epoch를 바인딩해 revoked tombstone을 기록하며 이미 확정된 누계를 되돌리지 않는다. `MsgBumpOrderEpoch`는 자기 epoch를1 증가시킨다. 두 메시지는 expiry/request-id/동일재시도·다른본문 충돌을 S1 방식으로 처리하고 자산 이동0이다. revoke는 대상이 처음 체인에 오더라도 `(owner,epoch,order_id)` binding을 그 hash로 만든다. 새 서명 body로 같은 ID 우회0. epoch/key 변경은 기존 order 전체에 적용한다.

operator epoch는 사용자 epoch와 별개다. 고정 genesis admin만 MsgRotateSettlementOperator를 사용한다. expected_epoch==current, new key는 별도 등록 ML-DSA operator, epoch+1 overflow 금지. LastBatch/과거 receipt는 유지한다. old operator의 신규 효과 TX는 거절되고 current operator가 과거 성공을 조회/재시도해도 추가 지급0이다. old epoch **미확정** Batch는 새 epoch로 재작성하지 않는다. 실패 증거·close·정정 후 새 epoch 명령으로만 재개한다. 회전 중 신규 매칭을 닫고 단일 writer를 유지한다. admin 경로는 로컬 CLI 테스트 전용이며 REST/UI 관리 API를 추가하지 않는다. 분산 fencing 검증으로 환산하지 않는다.

## 8. 의존 fill 폐쇄·WAL

S2의 무방향 owner 연결 성분 폐기를 재사용하지 않는다. fill 생성시 아래 의존을 원 outbox/WAL에 명시한다. source는 더 이른 아직 pending fill이고 edge는 source→현재 fill이다. 저장 edge는 아래 네 domain별 최신 선행 fill의 합집합으로 한정한다(최대4개). 각 domain의 연쇄가 모든 이전 의존을 전이적으로 보존하므로 이차 크기의 전체 predecessor 목록이 필요 없다.

- 양측 order_hash 두 개와 **debit reservation domain `(owner,owner_epoch,asset)`** 두 개 각각에서 가장 최근 pending fill을 predecessor로 삼는다. 중복을 제거해 원순서로 저장하며 replay 때 같은 최대4개 결과와 전이적 폐쇄를 비교한다.
- P는 어떤 edge의 재원이 될 수 없다. 같은 owner라도 서로 다른 debit asset/주문이면 그것만으로 의존 edge를 만들지 않는다. 누계·R/D 결정의 실제 이전 명령 seq도 원 WAL에서 보존하며 이전 fill domain 연쇄를 끊지 않는다.
- roots는 실패/VOID batch의 모든 fills 및 확인된 owner epoch/revoke event가 무효화하는 pending fills다. COMMITTED는 root/확장 후보에서 제외한다. 앞으로 향한 edge의 최소 고정점이 corrected 집합이다. 동일 reservation을 공유하는 후속 주문/잔량도 종료 대상에 넣는다.

freeze → in-flight 해소·close → 원 WAL에 VOID_BATCH/correction plan append → closure 계산 → 영향 open 잔량 종료 → 최신 같은 H의 C 기준 전체 R/D/P 재계산 → 원자 marker → 새 snapshot 순서다. correction_id=SHA256(frame(`NUS/S3/CORRECTION/V1`, canon({"context": Context, "void_batch": BatchIdentity, "snapshot_id": chain_snapshot_id, "root_fill_ids": lexicographically_sorted_root_ids}))); 재시도/재생은 같은 ID다. 원 WAL을 수정하거나 원 receipt·lifetime matched_qty를 감소시키지 않는다. corrected_qty와 settled_qty를 별도 기록하고 `settled+pending+corrected=lifetime_matched<=signed max_qty`를 유지한다. 정정량으로 예전 주문을 rematch0.

**비순환 해시:** `EngineState.corrections`는 after hash 없는 `CorrectionRecord` 전체를 append-only로 저장한다. 모든 상태 필드를 포함한 ENGINE_STATE 해시를 계산한 뒤, 같은 레코드에 그 해시를 추가한 완전한 `Correction`을 `CommandResult.correction_results`에 저장하고 COMMAND_RESULT 해시를 계산한다. 상태 해시의 임의 필드 제외/zero/null 정규화는 없다. 규범 계산 순서·재생·감사 검증은 `SCHEMA.md`의 정정 해시 절을 따른다.

affected_order_hashes는 corrected fill 참조 주문 + 이번에 종료한 open 주문을 원 admission_seq순·중복 없이 모두 포함한다. cancelled_order_hashes는 이번 open 잔량 종료만 포함한다. corrected fill IDs는 (command_seq,match_index)순이며 result/correction/outbox가 일치한다. 독립 surviving fill은 ID/양측 원서명/가격/원 command순서를 보존해 **VOID seq+1/원 VOID hash**로 새 배치에 담는다. 새 epoch가 되었다면 old-epoch fills는 독립 survivor가 될 수 없다.

`correction.json`은 F1→F2(같은 A 주문)→F3(C QUOTE 예약)를 정정하고 F4(B의 별도 확정 BASE에서 지출)는 보존하며 COMMITTED F0도 유지한다. 단순 owner 연결이라면 F4까지 지우는 오류를 검출한다. F4의 재원은 C_BASE이지 F1의 P_BASE가 아니다.

S2 WAL fsync→marker(temp fsync/rename/dir fsync)→공개, writer lock, unknown tail 증거 보존, 복구 중 외부 방송0을 유지한다. S3 WAL magic=`S3W1`, 72B header 및 payload<=16MiB. 내부 이력·정정 ID 배열은 페이지200/1000 한도로 자르지 않는다. 신규 ACK 전에 현재 전체 이력+모든 pending correction+최대 proof refs·최대 자릿수를 포함한 최악 정정 frame 및 marker/temp 공간을 예약한다. 큰 raw RPC evidence는 content-addressed fsync 파일로 저장하고 그 hash/길이/ref가 WAL commit에 포함돼야 한다. 참조 누락시 성공 복구0. 1000/1001 fill·200/201 orders·0/25bps 경계 회귀와 16MiB/16MiB+1을 유지한다. 예약 소진은 새 ACK 전 거절, 이미 승인된 정정에는 전용 예약 사용, IO 불명은 RECOVERY_REQUIRED다.

WAL/marker/snapshot 공동 과거 롤백을 로컬 해시만으로 항상 탐지할 수 없으므로 독립 클라이언트 ACK ledger와 대조한다. 이 한계는 계속 LOCAL_FSYNC이고 분산 내구성 인수가 아니다.

## 9. 검증·전달

명세 oracle/strict Go codec/실제 CIRCL fixture·serialized bytes 검사는 A 결과다. 실제 SDK settle/close/회전/revoke, Go/Rust/TS 제품 통합, 4검증인, 브라우저, crash IO, main CI/독립 G/J는 `acceptance.json`대로 **NOT_RUN**이다. A 모델 PASS를 제품 PASS로 복사하지 않는다.

S3-AT01~09 모든 variant를 새 S3 상태에서3회, 각 crash 재생은2회 수행한다. fault18개와 세부 변형은 `faults.json`. 각 oracle에는 1건의 부호/누계/중복/높이/의존 누락 결함을 주입해 실제 FAIL 검출을 확인한다. 같은 블록 순서는 barrier/TX index, HTTP 동시12건과 같은 요청 재시도12건, 오류/성공·SDK gas/자산 효과를 분리한다. RPC headers/완전 JSON/체인 확정/엔진 적용 지연을 별도로 측정한다.

각 실행은 code/tree·contract/config/vector/lock·실제genesis·binary hash, command/version/환경, H/index/TX/batch/fill ID, raw input/output·expected diff, PASS/FAIL/NOT_RUN을 남긴다. B/C는 A의 **두 심사가 끝난 exact head/tree/manifest**를 소비하며 자기 lock/code/tree를 별도로 추가한다. A branch는 I 이전 main 병합하지 않는다. 최초 실제 receipt 뒤 작업량/가스/용량/대기 재추정 기준은 ADR에 있다. 원래 T01~T16 full PASS0/16을 이번 계약만으로 올리지 않는다.
