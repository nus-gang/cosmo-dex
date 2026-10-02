# S2 실행 계약 1.0.0-rc1

2026-10-02 · CTO · [NUS-36](/NUS/issues/NUS-36). Security → QA 검토 후보이며 두 단계 완료 전 의존 구현을 열지 않는다. 승인 근거: [S2 Plan 6판](/NUS/issues/NUS-1#document-plan), revision `678c8648-d439-4f7b-9252-9d1cae507562`, 수락 `5890775a-9722-4af4-abf3-c9e1550453b1`. 기준 원격 main `24029b811e5ec798bbe57f769de3d3f254c90ab7`, tree `52478090ff2c3225ded1be53ec22efb2a47727d2`. 공유 작업 루트의 초기 HEAD는 기준이 아니다.

## 1. 규범·활성 프로필

이 문서, `SCHEMA.md`, `profile.json`, `schema.json`, `vectors/` 및 manifest가 S2 추가 계약이다. S0 `protocol/v1` rc4의 strict wire·서명·정수·오류 순서를 보존한다. S1 `protocol/s1`의 DIRECT 입출금/영수증을 두 자산으로 확장한다. OrderV1/CancelV1/WalletChallengeV1/FillIdentityV1의 tag·type·도메인을 변경하지 않는다. 새로운 REST/WAL JSON은 wire version이 아니라 S2 service schema version `1`이다. 불명 버전·다른 계약/config hash로 재생하거나 기본값을 채우지 않는다. 기존 schema 의미나 서명 필드 변경은 새 버전·벡터·CTO/Security/QA 심사 후 명시적 활성화가 필요하다.

단일 호스트 4검증인, 등록 시험 계정 2개, 시장 `DEVBASE/DEVQUOTE`, chain `nus-s2-dev-1`, module `x/exchange`, market config `1`, operator epoch `1`. 시장/설정은 실행 중 불변이다. S1 DB를 업그레이드하지 않고 새 genesis, 별도 node home·API journal·engine journal을 사용한다. 경로 기본은 `.runtime/s2/`; S1 home 또는 hash와 같으면 시작 거절. 사용자당 BASE/QUOTE 각 `1000000000000` atoms, GAS `1000000000` atoms, C/E=0으로 시작한다. 자산별 총 공급과 검증인 GAS 배분은 실제 genesis/manifest에 고정한다. 주문 재원은 실제 예치로 만든 C만이다.

BASE/QUOTE decimals=6, lot=1000 BASE atoms, lot-tick=1 QUOTE atom. q/p=1..1000000, 주문당 quote<=10^12 atoms, owner별 open 주문<=100, 시장 전체 open<=200. quote=q*p, base=q*1000. 최소 가격 tick=0.001 QUOTE/BASE. 모든 정수는 정규 십진 문자열, atoms U128, q/p/height/epoch/seq U64, match_index/cap U32, 중간 계산 U256 checked. 소수 입력의 반올림·TS Number 금지. DEVGAS는 별도 자산이다.

기본 fee version `1`=0bps. 부정/산술 시험 전용 version `2`=25bps는 별도 새 실행에서만 활성화한다. RECEIVE_ASSET_V1: 매수 BASE·매도 QUOTE 수취액에서 fill별 ceil, 양수 fee>=receive 거절. active_bps<=signed max_fee_bps, active 0..10000, signed cap U32 전체. 기본 cap=0, fee25 fixture cap=25. 분할 수수료를 합계에 한 번만 계산하지 않는다. S2 수수료는 잠정 fee 필드에만 기록하며 체인 T/C를 변경하지 않는다.

신규 Order: `2 <= expiry_height-observed_height <= 1000`; Wallet 기본 +100 blocks. 높이 합 overflow 거절. 대기 maker는 매칭 전에 h<expiry를 다시 검사하며 h==expiry에 미체결 잔량만 만료한다. 기존 잠정 D/P는 만료로 해제하지 않는다. Cancel은 기존 exclusive h<expiry, 여유 2 규칙을 적용하지 않는다. 2 blocks는 개발망 접수 여유이며 S3 정산 지연 보장이 아니다. S2에 생성된 outbox는 자동 S3 정산 입력으로 승격하지 않는다.

## 2. 체인 snapshot과 신선도

Chain은 단일 committed state H에서 두 계정의 등록 key type/raw key, owner epoch, account_number/sequence, 자산별 bank/C/GAS, 시장 설정과 자산별 module 보관량/공급을 함께 반환한다. GET 조각을 서로 다른 높이에서 조립하지 않는다. owner는 raw20의 canonical padded base64; 표시 주소는 nus Bech32. 공개키는 실제 등록 ML-DSA-65 raw1952. 검증된 등록값을 클라이언트가 덮어쓸 수 없다.

`ChainSnapshot` schema의 context와 body를 해시하여 snapshot_id를 만든다. 이 hash는 신뢰 RPC 응답의 동일성 증거이며 light-client/암호학적 상태 증명은 아니다. 원시 ABCI 응답 및 H의 block header/hash/time을 증거로 저장하고 요청·응답 height=H, chain/genesis/config/version과 owner 집합을 대조한다. H header의 app_hash를 H 실행 후 상태 root라고 부르지 않는다. proof 모드를 추가하면 실제 commit의 높이 관계를 별도 검증한다. S2는 승인된 신뢰 로컬 RPC 경계를 유지한다.

이벤트 cursor는 journal에 저장한 H에서 H+1 순으로 진행한다. H+2만 온 경우 C를 즉시 바꾸지 않고 `CATCHING_UP`; 빈 블록도 H+1 snapshot을 소비한다. 동일 H+동일 id 재관측은 무효과, 동일 H+다른 id 또는 높이 역행은 `SNAPSHOT_CONFLICT`/`HEIGHT_REGRESSION`로 닫는다. 최초 부팅은 보관된 genesis 기준에서 현재 확정 높이까지 조회한다. 보관되지 않은 중간 높이는 snapshot 일부를 생략하지 않고 `SNAPSHOT_UNAVAILABLE`로 중지한다. 새 입금은 C만 증가하고 epoch는 유지한다. C 감소에는 검증된 직접 출금/epoch 증가 근거가 필수다.

상태 `OPEN|CATCHING_UP|STALE|CORRECTING|WITHDRAW_FROZEN|RECOVERY_REQUIRED`. 신규 주문/매칭은 OPEN만 가능하다. 폴링 1000ms, RPC timeout 2000ms, 마지막 성공 조회 age 및 block age 모두 <=5000ms, 미래 block time 허용<=1000ms, RPC catching_up=false, 연속 cursor, 동일 config, 불변식 PASS를 모두 요구한다. >=가 아닌 초과 5000ms부터 STALE. 신선도 실패를 새 C=0으로 표시하지 않는다. 마지막 관측 높이·block_time·query_latency_ms·snapshot_id·stale 사유를 표시한다. 시간은 재생 결정 시 journal에 저장된 값을 사용하며 재생 중 현재 시계로 결과를 바꾸지 않는다. 운영 SLA/네트워크 안전성 보장은 아니다.

## 3. 서명·중복·접수

ML-DSA-65 pure/빈 context, raw owner=SHA256(pk)[:20], ORDER/CANCEL/WALLET_AUTH 도메인과 length frame은 S0 그대로. TX는 SDK DIRECT이며 ORDER frame과 교환하지 않는다. `vectors/signed.json`은 별도 합성 genesis의 S2 서명 fixture다. 실제 runtime genesis와 혼합하지 않는다. 서명 재현성은 테스트 생성용이며 제품 검증은 유효한 randomized signature도 허용한다.

신규 접수 순서: resource/canonical/presence → version/context → 등록키/owner → 실제 서명 → durable ID binding/기존 receipt → epoch/expiry → 시장/fee → 신선도·가용액·open 한도 → sequencer 전이 → 영속 commit → 응답. 미연결 등록키/신선도는 허용으로 바꾸지 않는다. 중복 확인 전 인증은 저장된 실제 등록키로 수행할 수 있으나 새로운 효과는 OPEN에서만 허용한다. 키는 S2에서 불변이다. 구 epoch 성공 재시도는 동일 결과 영수증과 현재 정정 상태를 함께 돌려준다.

Order binding key=(genesis,owner,owner_epoch,order_id), value=order_hash. 같은 본문 재시도는 최초 `CommandReceipt` 그대로 + 최신 entity view; 재예약/새 command_seq 없음. 다른 본문은 ID_CONFLICT. 첫 성공한 로컬 명령의 binding/receipt는 GC하지 않는다. canonical/auth 단계 거절은 binding 생성 없음; sequencer에 들어간 시장/잔고 등 결정적 거절은 명령/결과로 저장하고 같은 본문 재시도에 동일 거절을 반환한다. 저장 전 timeout은 `SUBMISSION_UNKNOWN`; 조회·같은 bytes 재시도만 허용한다.

Cancel binding key=(genesis,owner,cancel_nonce), value=cancel_hash. 대상 order_hash와 owner/epoch를 일치시킨다. 미존재는 ORDER_NOT_FOUND, hash 불일치 ID_CONFLICT. 이미 terminal인 주문에 새 유효 nonce로 취소하면 성공 no-op이며 기존 D/P 유지. stale 상태에서도 인증된 취소는 마지막 관측 H에서 h<expiry가 성립하면 예약 감소만 허용하고 `fresh=false`를 표시한다; 서명된 취소의 실제 현재 높이 유효성을 보장한다는 의미는 없다. disk/replay 오류 중에는 로컬 취소 성공도 금지하고 직접 TX 경로를 안내한다.

접수 성공 이름은 `LOCAL_ACCEPTED`, `durability=LOCAL_FSYNC`, `replicated=false`. 영수증은 command_seq/result_hash/journal_commit_hash/observed_height/snapshot_id를 반환한다. 이는 운영 설계서의 장애 영역 간 durable `ACCEPTED`가 아니다. 취소 entity 상태는 `CANCELLED_OFFCHAIN`; fill은 `PENDING|CORRECTED`뿐이다. 체인 입출금의 COMMITTED는 별도 화면 영역이며 로컬 주문/체결에 사용하지 않는다. HTTP 201은 로컬 명령 저장 완료, 200 동일 receipt 재조회, 연결 유실/202는 UNKNOWN; HTTP 코드만으로 경제적 결과를 판단하지 않는다.

## 4. 매칭·예약·잠정 수취

단일 sequencer가 order/cancel/expiry/snapshot/withdraw-freeze/correction 모두에 전역 command_seq를 부여한다. 같은 가격은 최초 성공 접수 seq FIFO, maker 가격으로 체결한다. BUY 높은 가격, SELL 낮은 가격 우선. 부분 체결 maker는 원 우선순위 유지. STP는 `CANCEL_TAKER_REMAINDER`: 자기 주문이 최우선이면 그 주문을 건너뛰지 않고 taker 잔량 종료, 앞서 발생한 타인 fill 유지. GTC=1, 가격 제한 IOC=2만 허용한다.

A=C-R-D>=0, P는 가용액/재주문/출금 재원이 아니다. 매도 R/D=q*1000 BASE, 매수 R/D=q*buy_limit_ticks QUOTE. fill q만큼 R→D 이동, buyer P_BASE+=base-fee_base, seller P_QUOTE+=quote-fee_quote. 가격 개선분도 D에 남는다. 미체결 취소/IOC/만료는 R만 반환한다. 잠정 fee는 C/T에 반영하지 않는다. maker/taker의 수량 누계와 모든 중간값을 checked 연산하며 fill 하나라도 정책 부적합하면 그 후보 fill을 생성하지 않고 taker 잔량을 `POLICY_REJECTED_REMAINDER`로 종료한다; 앞선 유효 fills는 보존한다.

OrderBook-rs callback은 명령별 버퍼에 수집하여 canonical MatchOutcome으로 정규화한다. IOC Err에도 callback fills가 있으면 그 fills를 1회 반영하고 미체결 R만 반환한다. callback/return에 같은 fill이 함께 있으면 하나로 계상한다. 식별 불일치·누계 초과·한도 위반·예상치 못한 adapter error는 공개 상태/성공 응답을 내지 않고 RECOVERY_REQUIRED. 시계/ID는 주입한다. 원장·ID binding·WAL·cursor·서명 권한은 upstream에 위임하지 않는다.

fill_id=SHA256(frame(NUS/FILL_ID/V1, canonical FillIdentityV1(chain_id,market_id,operator_epoch,command_seq,match_index))). match_index=0부터 증가. journal namespace에 genesis를 별도로 결합하고 다른 genesis의 동일 id를 합치지 않는다. 라이브러리 UUID는 제품 ID가 아니다. outbox의 fill/양측 서명 원본/최악 D/net P/fee/사용 snapshot/config hash/의존 ID를 같은 commit에 저장한다. `submission_enabled=false`, `export_state=HELD_S2`; 배치 seq를 할당하거나 체인에 제출하지 않는다.

## 5. 직접 출금·epoch 정정 (ADR S2-03)

일반 출금 준비는 로그인 owner 기준 명령으로 owner 신규 접수/매칭 동결→미체결 주문 전부 취소→D/P 확인을 하나의 sequencer 순서로 처리한다. D/P 중 하나라도 남으면 `UNSETTLED_HOLD`(정산 미구현/미정산 보류), TX를 자동 생성/서명하지 않는다. D/P=0이면 기존 S1 방식으로 사용자가 직접 서명·제출한다. 준비 실패 뒤 자동으로 주문을 부활시키지 않는다. 사용자가 명시적으로 준비를 취소해도 새 snapshot/불변식 검사 뒤 새 주문만 받는다. 준비는 출금 권한 토큰이 아니다.

직접 MsgWithdraw는 Exchange 허가 없이 체인이 처리한다. owner epoch는 **자산 공통**이므로 BASE 일부 출금도 그 owner의 QUOTE 주문을 무효화한다. 확정 snapshot의 E 증가를 관측하면 전체 신규 매칭을 CORRECTING으로 닫는다. Chain C가 최종 권위다. 다음 보수적 정정 정책은 S2에만 적용한다.

1. 이전 상태의 PENDING fills를 owner 간 무방향 그래프로 보고 epoch 변경 owner를 시작점으로 연결 성분의 owner 집합을 고정점까지 확장한다. 이 집합의 모든 open 주문·pending fill을 포함하므로 후속 의존 주문/다른 상대방까지 빠뜨리지 않는다. 범위가 넓어질 수 있음을 UI에 밝힌다.
2. 영향 주문을 원 접수 seq순으로 terminal 처리하고 미체결 R을 해제한다. 영향 fill을 (command_seq,match_index)순으로 CORRECTED 처리, 각 원본 D/P/잠정 fee를 정확히 역분개한다. corrected_qty는 lifetime filled_qty에서 빼지 않는다. 누계량 재사용·자동 rematch·기존 ID 재접수 금지. 양측에 reason=OWNER_EPOCH_CHANGED 또는 DEPENDENCY_CORRECTION과 원 fill ID/revision을 남긴다.
3. 동일 H의 전체 새 C/E/등록키/설정을 적용하고 원장 합산과 A>=0을 재검증한다. 영향을 받지 않은 주문 FIFO/ID는 보존한다. 정정 계획·원본/결과 hash·새 snapshot/cursor·outbox 상태를 하나의 journal commit으로 기록한다. 중간 crash면 공개하지 않고 같은 correction을 재생한다.
4. 영속화와 신선도·cursor 검사 뒤 OPEN으로 재개한다. 불변식/이벤트 근거 불일치면 RECOVERY_REQUIRED를 유지한다. 동일 snapshot 재관측과 재시작은 정정 효과 1회다. 클라이언트는 revision 증가로 양측 정정을 수신한다.

정정은 체인 정산 실패 receipt가 아니라 **S2에서 제출하지 않은 잠정 체결의 폐기**다. S3의 in-flight 배치가 존재하면 이 정책을 그대로 적용할 수 없다. 실제 출금↔정산 블록 순서 T04 또는 정산 의존 T16 전체 PASS를 뜻하지 않는다. 직접 출금 이후 D/P를 임의 초기화하거나 원본 기록을 삭제하지 않는다.

## 6. WAL·snapshot·복구

단일 프로세스 writer의 OS advisory exclusive lock을 journal 디렉터리 전체에 잡고 생존 동안 유지한다. 두 번째 프로세스는 `WRITER_ALREADY_RUNNING`으로 종료한다. PID 파일만으로 대체하지 않는다. 네트워크 파일시스템/자동 failover/분산 fencing은 범위 밖이다.

명령은 비공개 후보 상태에 적용하고 canonical JSON JournalRecord에 원문·signature hash·등록키 증거·snapshot/context·기록시각·명령 seq·이전 commit hash·예약 변화·매칭 결과·전체 복구에 필요한 상태/결과·fills/outbox/외부 event를 담는다. JSON hash 규칙은 schema 문서 참조. 권장 파일 framing은 `S2W1` 4B + payload_length u32be + payload SHA256 32B + 앞40B SHA256 32B + canonical payload이며 총 header 72B다. header 검증 전에 length를 신뢰하지 않고 payload 최대 16777216 bytes를 적용한다. record hash=SHA256(전체 frame).

append/fsync WAL → 독립 commit marker 파일에 seq/record hash/end offset 원자 교체(temp fsync, rename, directory fsync) → 상태 공개/영수증 응답 순서. marker와 payload/outbox는 같은 논리 commit 경계다. marker보다 뒤의 완전 record는 UNKNOWN tail이며 복구 시 증거 보존 후 같은 결정으로 완료하거나 실패 정지한다; 성공 응답으로 간주하지 않는다. marker가 가리키는 frame 부재·checksum/hash/seq 불일치, 완료 frame 손상, header 손상은 RECOVERY_REQUIRED로 중지한다. marker의 seq 이하를 자동 truncate/빈 genesis 성공 복구하지 않는다. fsync 의미/OS·파일시스템과 macOS full-fsync 미검증 한계를 manifest에 기록한다.

snapshot에는 주문장 FIFO·C/R/D/P·pending fees·ID binding·receipt·fills/outbox·현재 chain snapshot/cursor·last command_seq/record hash/end offset를 함께 저장하고 content hash를 검증한다. snapshot temp fsync/rename/dir fsync 후 marker와 일치하는 위치부터 tail 재생한다. 이번 단계 WAL/tombstone GC 없음. 재생은 저장한 입력·시계·설정·ID로 결과 hash를 검산하고 외부 전송 0건이어야 한다. 손상된 snapshot은 WAL 전체가 온전할 때만 원본 보존 후 재구성한다.

부분 header/payload 및 불확실 tail는 원본 WAL/marker/snapshot을 별도 evidence 디렉터리에 fsync 보존한 뒤에만 수동/명시적 복구 절차로 처리한다. 로컬 marker와 WAL 둘 다 동일 과거 상태로 롤백된 상황은 로컬 파일만으로 탐지 보장할 수 없으므로 외부 시험 ACK ledger와 마지막 receipt seq/hash를 대조한다. 이러한 한계를 분산 durable ACK/T08/T10 PASS로 숨기지 않는다.

## 7. API·인증·화면

`schema.json`의 객체는 모든 필드 required, 추가/중복 key 금지, null은 명시된 곳만 허용한다. Order/Cancel은 canonical wire base64와 signature base64를 제출하며 server가 디코드·검증한 바이트를 보존한다. JSON wrapper의 숫자/hash/bytes 표현은 S0 규칙과 같다. 요청 최대 16384B(서명 wrapper 포함); account는 token에서 결정하며 요청 body의 owner로 권한을 대체하지 않는다.

| 경로 | 권한·계약 |
|---|---|
| GET /s2/network, /s2/status | 공개; Network/Status, 계약/config/genesis hash와 접수·신선도 |
| POST /s2/auth/challenges | owner/origin/audience 입력; 서버 nonce32, WalletChallengeV1 반환 |
| POST /s2/auth/sessions | challenge wire+signature; 원자 nonce 소비 후 owner 세션 |
| POST /s2/orders, /s2/cancels | 해당 owner 세션 + 실제 Order/Cancel 서명; CommandReceipt |
| GET /s2/book | 공개 BookSnapshot; owner/order hash/서명 제외 |
| GET /s2/me | LedgerView + 자기 orders/fills, 하나의 seq/revision/snapshot |
| GET /s2/me/orders/{order_id}?epoch=... | 자기 entity 상태 + 최초 receipt |
| GET /s2/me/commands/{kind}/{id}?epoch=... | ORDER는 order_id+epoch, CANCEL은 nonce; receipt 또는 NOT_FOUND_AT_SEQ |
| POST /s2/me/withdraw-prepare, /s2/me/withdraw-abort | 로그인 owner만; 로컬 freeze/cancel 또는 재개 명령. 금액 이동 권한 없음 |
| /s1/txs 및 S1 receipt 경로 | S2 network에서 동일 DIRECT 검증 방식, 두 denom 허용; S1 실행과 namespace 분리 |

WalletChallenge TTL<=120s, issued<=now<expiry; 세션 TTL=300s, owner/origin/audience/genesis에 결합한 무작위 bearer token은 메모리에만 보관한다. 재시작/로그아웃/계정 전환에 세션 폐기, challenge nonce는 한 번만 원자 소비. 세션 재연결은 재인증한다. audience는 exchange-api만 활성. 공개 데이터 이외는 모든 GET에도 세션 필요하며 타 owner=403, 미존재와 타인의 주문 유무를 구분해 누설하지 않는다.

S2 로컬 예외를 명시적으로 추가한다: `http://127.0.0.1:5173` 및 `http://localhost:5173` 두 origin만 정확 일치 허용; HTTP는 loopback/dev profile에만 한정. S0 HTTPS 규칙은 다른 profile에서 보존한다. 프록시·API는 loopback에 bind, CORS exact allowlist, wildcard 없음. challenge의 server_origin은 브라우저 origin이며 actual Origin과 같아야 한다. missing/foreign Origin의 개인 mutation은 거절. HTTPS 운영 인증 승인이 아니다. key/seed는 서버 전송·로그·localStorage에 저장하지 않는다. 탭 종료 후 키 복구 불가를 안내한다.

REST polling 1초, 공개 book/개인 view 모두 monotonic stream_seq와 entity revision을 제공한다. full snapshot은 한 committed engine seq에서만 생성한다. 재접속은 전체 snapshot 재조회; 서로 다른 network/genesis/account_generation의 응답은 폐기한다. 같은 stream_seq 다른 content hash는 오류, 낮은 seq/revision은 무시한다. 토큰 만료·RPC 단절·높이 역행·5초 stale에 입력을 닫고 마지막 관측값임을 표시한다. UNKNOWN을 실패/새 주문 ID로 바꾸지 않는다. C/R/D/P/A 단위와 잠정/정정/오프체인 취소를 항상 구분한다. P에는 “잠정 체결 자산은 아직 쓸 수 없음”을 표시한다. 전체 WS는 미제공이다.

오류: S0 우선순위를 보존하며 새 오류표 `errors.json`을 따른다. 인증 실패 응답에 타인 상태/등록키를 포함하지 않는다. 상태 불명은 마지막 관측 height nullable, 경제효과 무효라고 단정하지 않는다. 순수 거절은 `REJECTED`, 미연결/IO 불명은 `SUBMISSION_UNKNOWN`이며 성공 receipt를 덮어쓰지 않는다.

## 8. 산출물·검증 게이트

manifest `contract_sha256`은 S2 규범/도구/벡터와 정확한 S0/S1 기준 파일 해시를 결합한다. B~J는 승인된 PR head와 이 hash를 함께 pin한다. A 승인 후 B/C는 동일 head에서 브랜치를 만들거나 해당 계약 commit을 보존하여 소비하고 I가 최종 main에 통합한다. 각 실행은 code SHA/tree, contract/config/vector/lock/genesis/binary hash, 활성 fee profile, 도구·OS·시각·명령·결과·자원을 기록한다. A의 합성 genesis는 runtime 인수가 아니며 실제 genesis는 B/F가 생성 후 hash를 채운다.

필수 제품 시험은 승인 Plan S2-AT01~09 그대로다. `acceptance.json`은 담당·필수 원시 증거·실행 여부를 제공한다. A 자체 검사는 산술·schema·hash·합성 서명 입력·CIRCL fixture 검증이다. Go/Rust/TS 제품 통합 서명 검증·실제 예치/서비스/WAL crash/브라우저·main QA는 B~J에서 실행하며 A 통과만으로 PASS 처리하지 않는다. 기존 T01~16 full PASS 0/16 기록을 유지하고 J가 전체 원 조건을 실제 검증한 경우에만 갱신한다. 실제 정산 T04/06/07/09/16, 분산 T08/10, 독립 회수 T13, WS T14 전체는 S2 부분시험으로 대체하지 않는다.
