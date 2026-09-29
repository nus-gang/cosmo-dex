# 공통 프로토콜 v0.1.0-draft — M0 검토용

작성: CTO, 2026-09-28. 상태: 제안. 본 구현 승인 또는 기술 검증 완료를 뜻하지 않는다.
근거: [개발 설계서 v1.0](/api/attachments/9c378275-dcec-4402-8478-4bf7d26af911/content) 6–12쪽, [아키텍처](/api/attachments/d157936f-f4b3-45e4-849a-f68621a7b90d/content). 출금·취소와 수수료는 더 구체적인 개발 설계서를 우선한다.

## 규범과 변경 관리

M0 후보 계약을 일찍 공개한다. Go/Rust/TS 통합 성공이나 SDK 지원은 아직 주장하지 않는다. CTO가 계약 합의를 관리하고 Security가 골든 벡터를 조정한다. Chain·Exchange·Settlement·Wallet이 바이트와 상태 의미를 검토하고 SRE가 복제/키/회수 경로를 검토한다. Security·Tester의 독립 검토는 각자의 업무에 기록하고 CTO는 링크와 결과만 통합한다.

wire protocol_version=1. 문서의 draft 버전과 wire 버전을 구분한다. 서명 바이트·필드 의미·정수 폭·도메인이 바뀌면 wire 새 버전과 새 벡터가 필요하다. 알 수 없는 버전은 거절한다. market_config_version과 fee_policy_version은 immutable 이력이며 동일 번호 덮어쓰기를 금지한다. 변경은 신규 접수 중지 → 미정산 해소 → 구 주문 무효화 → 새 버전 활성화 순서로 한다. 지원 버전과 전환 높이를 manifest로 공유한다.

## 공통 형식

아래 필드 순서는 Protobuf tag 1부터 연속 증가하는 제안이다. 단일 필드는 모두 필수이며 optional presence를 사용하고, 기본값 0도 정확히 1회 명시적으로 인코딩한다. 단일 필드의 중복 tag는 값이 같아도 금지한다. repeated message에는 optional presence를 적용하지 않으며 원소마다 같은 tag와 wire type LEN(2)를 반복한다. tag는 비감소 순서이고 동일 repeated tag의 원소들은 한 연속 구간에 배치한다. 다른 tag를 사이에 넣거나 앞 tag로 되돌아가는 인코딩은 거절한다. 빈 배열은 해당 tag를 0회 인코딩하며 길이 0인 message 1개로 대신하지 않는다. 중첩 message도 이 규칙을 재귀적으로 따른다.

일반 Protobuf deterministic 옵션만으로 정규성을 가정하지 않는다. 최소 길이 varint만 허용하며 unknown tag/필수 단일 필드 누락/잘못된 wire type을 거절한다. 수신 바이트의 구조·중복·원소 순서를 검사하고 canonical decode/reencode 결과와 비교하여 다르면 거절한다. 수신 배열을 정렬해 잘못된 순서를 정상 입력으로 바꾸지 않는다. map/float/signed integer를 사용하지 않는다. 배열 원소 순서는 아래 Batch 규칙을 따른다. 이 profile의 코드 생성 적합성은 3언어 벡터 합의 전 미검증이다. repeated message의 원소별 동일 tag 인코딩 근거는 [Protobuf 공식 인코딩 규칙](https://protobuf.dev/programming-guides/encoding/#repeated-elements)이며 연속 배치와 정렬은 이 계약에서 추가한 제약이다.

타입: U32=uint32, U64=uint64, S=제한된 ASCII 문자열, H=bytes 32바이트, A=주소의 raw bytes, PK=공개키 raw bytes, ID=bytes 32바이트. S는 1–128바이트 `[A-Za-z0-9._:/-]+`; origin/audience는 별도 규칙 적용. A 길이·PK 형식은 Chain/Wallet의 실제 사용 태그 실증 후 DEC-07에서 고정한다. 그 전 서명 벡터의 owner는 모의 식별자이며 실제 주소라고 주장하지 않는다. H의 JSON 표현은 소문자 hex 64자; bytes는 base64 RFC4648 padding 포함; U32/U64/금액은 JSON에서 정규 십진 문자열 `0|[1-9][0-9]*`. 음수, +, 공백, 지수, 소수, 선행 0 거절. TS Number 변환 금지.

서명 입력은 `u32be(domain byte length)||ASCII(domain)||u64be(body byte length)||canonical body`. 도메인 문자열은 정확히 `NUS/ORDER/V1`, `NUS/CANCEL/V1`, `NUS/WALLET_AUTH/V1`. 사용자 서명은 ML-DSA-65 pure 모드·빈 context 후보이며 라이브러리 일치 검증을 DEC-07에서 수행한다. 임의 prehash 서명은 금지한다. order_hash=SHA-256(order 서명 입력); 서명 자체는 해시에 포함하지 않는다. 공개키에서 주소 파생 및 기존 계정 키 일치 검사는 선택한 SDK 규칙을 그대로 고정한다. 미등록 키는 v1 후보에서 먼저 계정 등록 TX 확정을 요구한다(제품 마찰은 Wallet 검토).

## OrderV1

|tag|필드|타입·제약|
|---|---|---|
|1|protocol_version|U32, 1|
|2|chain_id|S|
|3|genesis_hash|H, 승인 manifest에 고정된 genesis 원본 바이트의 SHA-256|
|4|exchange_module_id|S, `x/exchange`|
|5|market_id|S|
|6|market_config_version|U64, >0|
|7|owner|A|
|8|owner_pubkey|PK|
|9|order_id|ID, 사용자 생성 고유값|
|10|owner_epoch|U64|
|11|side|U32, BUY=1 / SELL=2|
|12|limit_price_ticks|U64, >0|
|13|max_qty_lots|U64, >0|
|14|max_fee_bps|U32, 0..10000; 실제 fee < 수취액도 검사|
|15|fee_asset_policy_id|S, 후보 `RECEIVE_ASSET_V1`|
|16|expiry_height|U64, >0|
|17|order_type|U32, LIMIT_GTC=1 / LIMIT_IOC=2|

SignedOrderV1은 order(1, message), signature(2, bytes). 무제한 시장가는 지원하지 않는다. IOC 미체결 잔량은 시퀀서에서 취소한다. `(owner,owner_epoch,order_id)`는 첫 order_hash에 영구적으로 결합한다. 동일 해시 재제출은 기존 결과를 반환하며 예약하지 않는다. 다른 해시는 충돌이다. 최초 체인 정산에 본문·서명을 포함하고 이후에는 검증된 바인딩만 참조할 수 있다. 상태 삭제는 재생 불가능성 검토 전 금지한다.

만료 후보: 실행 블록 높이 h < expiry_height일 때만 유효하며 h == expiry_height부터 거절한다. 매칭 시점은 권한을 연장하지 않는다. height=0과 무한 만료는 금지한다. 안전 여유 높이/배치 한도 숫자는 DEC-05 실측 대상으로 남긴다.

## CancelV1

순서/tag: protocol_version(U32), chain_id(S), genesis_hash(H), market_id(S), owner(A), owner_epoch(U64), order_id(ID), order_hash(H), cancel_nonce(ID), expiry_height(U64), exchange_module_id(S).
마지막 module 필드는 개발 설계서 대비 도메인 결합 강화 제안이다. 별도 CANCEL 도메인으로 서명하고 공개키는 등록된 계정 키로 검증한다. nonce를 owner와 함께 1회 사용 처리한다. 같은 nonce·본문 재시도는 기존 취소 결과, 다른 본문은 충돌. 시퀀서 기준 확정 관측 높이에서 h < expiry_height를 검사하고 사용 높이를 WAL에 기록한다. 오프체인 취소는 이후 매칭만 막고 이미 잠정 체결된 D를 풀지 않는다. 응답 `CANCELLED_OFFCHAIN`은 온체인 무효화를 의미하지 않는다. 강제 무효화는 사용자가 서명한 MsgRevokeOrder/MsgBumpOrderEpoch TX의 확정을 요구한다.

## BatchV1 / FillV1

Batch tag 순서: protocol_version(U32), chain_id(S), market_id(S), operator_epoch(U64), batch_seq(U64), previous_batch_hash(H), batch_id(H), new_signed_orders(repeated SignedOrderV1), fills(repeated FillV1), genesis_hash(H), exchange_module_id(S), market_config_version(U64).
추가 tag 10–12는 환경·설정 결합 강화 제안이다. 최초 seq=1, previous hash=32바이트 0. seq는 시장별로 운영자 교체에도 계속 증가한다. new_signed_orders는 order_hash 바이트 사전순, 중복/미사용 증빙 금지. fills는 (command_seq,match_index) 엄격한 오름차순이며 동일 tuple도 거절한다.


### 반복 필드 문서 예제와 기대 판정

아래는 전체 정상 Batch에서 tag 8/9 구간만 표시한 구조 예제다. 나머지 필수 필드와 각 message의 서명·참조·제약은 유효하다고 가정한다. A/B는 order_hash가 각각 0x11 32바이트 / 0x22 32바이트인 SignedOrderV1, F/G는 tuple이 (10,0)/(10,1)인 FillV1이며 A/B가 실제 사용되는 증빙이다. 표는 canonical 구조의 기대 판정이고 실행된 서명·wire fixture 검증 결과가 아니다.

|입력 구간 또는 변경|기대 판정|근거|
|---|---|---|
|8:A, 8:B, 9:F, 9:G|허용|각 repeated에 정상 2원소, 연속 tag와 원소 순서 준수|
|단일 tag 5 batch_seq를 같은 값으로 2회 인코딩|거절|단일 필드는 정확히 1회|
|8:B, 8:A, 9:F, 9:G|거절|order_hash 역순|
|8:A, 8:B, 9:G, 9:F|거절|fill tuple 역순|
|8:A, 9:F, 8:B, 9:G|거절|repeated 구간 분리 및 tag 역행|
|8:A, 8:A 또는 9:F, 9:F|거절|주문 해시 또는 fill tuple 중복|
|tag 8 생략, 9:F, 9:G; 주문은 이미 체인에 등록됨|구조상 허용|빈 new_signed_orders는 0회 인코딩|
|tag 8/9 모두 생략|구조상 허용|빈 배열 표현. 빈 배치의 실행 허용 정책은 별도 합의 대상|
|길이 0인 tag 8 또는 tag 9 message 1개|거절|빈 배열이 아닌 불완전 원소로서 내부 필수 필드 누락|

자기참조 제거: BatchCore는 Batch에서 tag 7(batch_id)을 제외한 canonical bytes다. batch_id=SHA-256(frame(`NUS/BATCH_ID/V1`,BatchCore)). batch_hash=SHA-256(frame(`NUS/BATCH_HASH/V1`,canonical BatchV1)). previous_batch_hash는 직전 확정 batch_hash를 가리킨다. 정산의 권한 서명은 운영자의 표준 체인 TX 서명이며 사용자 서명을 대체하지 않는다.

Fill tag 순서: fill_id(H), maker_order_ref(H), taker_order_ref(H), buyer_order_ref(H), seller_order_ref(H), execution_price_ticks(U64), quantity_lots(U64), fee_policy_version(U64), command_seq(U64), match_index(U32).
refs는 order_hash다. maker/taker 집합은 buyer/seller 집합과 같아야 하며 v1 수수료 차등 근거가 아니다. 각 fill의 market/operator는 batch에서 상속한다. fill_id=SHA-256(frame(`NUS/FILL_ID/V1`,tuple protobuf(chain_id S tag1, market_id S tag2, operator_epoch U64 tag3, command_seq U64 tag4, match_index U32 tag5))). 원본의 결정적 tuple을 해시한 형태다. 사용자 동일 self-trade는 거절한다.

체인은 allowlist/현재 operator_epoch, 연속 seq/hash, 모든 주문의 서명·epoch·취소·만료·설정·가격·누적 수량·수수료를 검사한다. 동일 배치 재요청은 저장된 seq/id/hash와 동일하면 기존 확정 결과를 반환하고 새 이동은 없다. 같은 seq 다른 hash, 중복 fill, 누락 seq는 거절한다. 이전 배치 영수증/SeenFill을 GC하지 않는다(보존기간은 후속 검토).

C_start에서 사용자·자산별 gross_debit 합계를 먼저 검사한다. 배치 내 수취액 재사용 금지. 한 fill 실패 시 배치 전체 롤백. RPC timeout은 거절 증거가 아니다. Settlement는 체인 seq/hash/영수증을 먼저 조회하고 결과 불명 상태에서 D/P를 해제하거나 새 ID로 다시 정산하지 않는다.

## WalletChallengeV1

tag 순서: protocol_version(U32), chain_id(S), server_origin(string), audience(string), owner(A), challenge_nonce(ID), expiry_time(U64), genesis_hash(H), issued_at(U64).
시간은 Unix seconds UTC. nonce는 서버 생성 32바이트 난수, 세션·owner·origin·audience에 저장하여 성공 검증과 동시에 원자 소비한다. 후보 TTL 120초, 서버 시간으로 issued_at <= now < expiry_time, expiry-issued <=120 검사. 동일 nonce 재사용 거절. origin은 HTTPS의 정규 scheme://host[:nondefault-port], 경로/쿼리/fragment 금지, 서버 allowlist와 정확 일치. audience는 정해진 `exchange-api` 또는 `private-ws`. 지갑은 요청 origin과 서명 대상 origin을 대조하고 사용자에게 표시한다. 토큰은 owner/audience/expiry에 묶고 개인 WS 재연결 때 유효성 재검사한다. 로그인 서명으로 주문·취소·TX를 승인할 수 없다. localhost 예외는 합성 M0 fixture만 허용한다. 세션 수명·폐기 저장소는 Settlement/Security 검토.

## 정수·원장 계약

후보 범위: 금액 atoms 0..2^128-1, lot/tick/높이/seq U64. 중간 곱은 256비트 checked 연산 또는 동등 arbitrary precision으로 계산한 뒤 명시적 상한 검사. wire 금액이 필요하면 bytes 16바이트 unsigned big endian 고정(최상위 0 유지). wrap/saturating 계산 금지. 시장 min/max lot·tick, denom과 lot당 atoms는 DEC-10 승인 설정으로만 제공한다. 사용자 소수 입력을 최소단위로 정확히 바꿀 수 없으면 반올림하지 말고 거절한다.

B=q*base_atoms_per_lot, Q=q*p*quote_atoms_per_lot_tick. seller_limit <= p <= buyer_limit. fee=ceil(receive_atoms*bps/10000)=(product+9999)//10000. 실험 후보는 최소 수수료 없음, bps=0일 때만 0 허용, 양수 bps면 올림하며 각 fill마다 부과한다. 분할 체결에 따른 총 수수료 증가를 화면에 설명하고 주문 cap은 각 fill 적용 bps 상한이다. 누적 수수료 상한이 필요하면 별도 서명 필드/버전 필요. fee >= receive이면 fill 거절. 자산별 합계 변화는 0: buyer(+B-fB,-Q), seller(-B,+Q-fQ), T(+fB,+fQ).

bank_balance(exchange,d)=sum C[u,d]+T[d]+U[d]. C/T/U>=0. A=C-R-D>=0, P는 A에 포함하지 않는다. 매수 D는 실제 체결가가 아닌 서명 한도가의 최악 입력액을 보류한다. 확정 또는 확정 거절과 재생이 끝나기 전 가격개선 차액을 풀지 않는다. U>0 격리·경보는 정상 확정 출금을 막지 않는다. 부족액은 신규 정산 중단 사유다.

출금 TX는 owner=서명자=수취인, epoch 증가와 잔고 차감이 원자적이다. settle→withdraw는 정산 후 잔고에서 출금, withdraw→settle는 구 epoch 배치 거절. 엔진은 직접 출금/epoch 변경 확인 시 해당 주문·잠정 체결을 동결하고 같은 확정 높이에서 C/R/D/P를 재생한다. 운영자 장애에도 직접 회수 경로를 유지하며 합의 중단 동안 확정 출금은 불가능하다.

## 검증 입력·승인 조건

Security 벡터에 필수 포함: 세 도메인 정상/교차 서명, genesis/owner/pubkey 불일치, unknown/단일 필드 중복/비연속 repeated/원소 역순/nonminimal protobuf, 2^53+1 JSON, U64/U128 경계, 곱 overflow, fee 0/1/동액 거절, 만료 h-1/h/h+1, 부분체결 누적 초과, 동일 ID 다른 본문, 배치 자기참조 제거, 중복 fill/seq, C_start 재사용, 출금 두 순서, nonce 동시 재사용. 확률적 서명은 signature bytes 동일성을 요구하지 않고 고정 서명 fixture의 검증 결과와 입력 바이트/해시 동일성을 요구한다.

현재 결과: 원본 대조 및 계약 초안 작성만 완료. 실제 ML-DSA 서명, 주소 파생, Go/Rust/TS 일치, 성능 시험 미실행. Security 골든 벡터와 담당별 피드백 합의 전 본 구현 착수 금지. 합의 시 manifest에 문서 해시·wire version·의존성 tag/lock·벡터 해시·검토 업무 링크를 기록한다.
