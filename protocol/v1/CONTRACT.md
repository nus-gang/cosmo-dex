# S0 계약 v1.0.0-rc1

CTO 결정, 2026-09-29. 상태: **계약 단계 Security·QA 검토 대기, 독립 최종 검증 전**. 승인 범위는 S0-A~H이며 제품 통합·실자산 정책이 아니다.

## 규범 및 버전

`m0-baseline.md`의 필드·프레임·보존식을 채택하며 아래 확정 사항이 그 문서의 후보/미정 문구보다 우선한다. 실제 tag/type은 `protocol.proto`와 `schema.json`을 따른다. M0 r2의 주문/취소/인증/배치 서명 바이트를 변경하지 않았다. wire version=1; rc는 문서 배포 버전이다. 이 rc가 검토 통과하면 동일 해시를 소비한다. 서명 필드/정수 폭/도메인 변경은 새 wire version, 새 벡터, 명시적 활성화가 필요하다. unknown version 거절. immutable config/fee version 덮어쓰기 금지.

## 정규 wire와 JSON

모든 singular tag는 정확히 1회, 0도 presence 필수. tag 비감소, minimal unsigned varint, unknown/duplicate/wrong wire/missing tag 거절. repeated message는 tag별 연속, 빈 배열은 0개 tag. 배열을 정렬해서 수신 오류를 복구하지 않는다. nested에도 동일 적용. 일반 protobuf decoder 단독 사용 금지: strict wire 검사 후 decode/reencode 동일성 확인. 크기 제한은 dev-config의 bytes/count/depth를 decode 전에 적용한다.

JSON 모든 정수(U32도 포함)는 `0|[1-9][0-9]*` 문자열. unknown/duplicate JSON key, 부호/공백/소수/지수/선행0, 범위 초과 거절. schema h=32-byte lowercase hex, a/pk/sig/atoms=RFC4648 canonical padded base64. 벡터의 `fields`에 쓰는 `hex`는 fixture 표현이며 API base64 규칙을 바꾸지 않는다. atoms wire는 16-byte unsigned big endian. 문자열 s는 baseline ASCII 제한; server_origin만 아래 별도 제한. SignedOrder.order는 object, repeated는 배열. map/float/signed integer 없음.

## 키·도메인·검증

ML-DSA-65 pure, FIPS context empty. raw pk=1952B, signature=3309B, owner=SHA256(raw pk)[:20]. 길이를 먼저 검사한다. SDK wrapper/hex 문자열을 해시하지 않는다. raw owner를 서명하고 표시용 Bech32는 HRP `nus`, lowercase canonical roundtrip만 허용한다. 다른 HRP, mixed case, 잘못된 checksum 거절. 주문은 확정 등록 계정의 key type 및 raw bytes와 동일해야 한다. 미등록 계정은 ACCOUNT_KEY_UNREGISTERED. Cancel/Wallet은 등록 키를 조회한다. 키 교체는 기존 주문을 살려두지 않고 epoch 무효화를 거쳐야 한다.

서명 frame은 u32be(domain length)||ASCII domain||u64be(body length)||canonical body. ORDER/CANCEL/WALLET_AUTH 및 BATCH_ID/BATCH_HASH/FILL_ID는 baseline 그대로. TX는 SDK SIGN_MODE_DIRECT 경로이며 이 frame과 교환 금지. TransferStableV1은 직접 ML-DSA frame이 아니라 체인 TX body 안의 메시지다. payment_hash=SHA256(frame(`NUS/PAYMENT_ID/V1`,canonical TransferStableV1)); 권한 서명은 TX이며 이 hash는 멱등성 전용이다.

검사 우선순위: RESOURCE_LIMIT → NON_CANONICAL_WIRE/INTEGER_RANGE → UNSUPPORTED_VERSION → CONTEXT_MISMATCH → KEY_LENGTH/ADDRESS_MISMATCH/ACCOUNT_KEY_UNREGISTERED/ACCOUNT_KEY_MISMATCH → INVALID_SIGNATURE → ID_CONFLICT → EPOCH_MISMATCH/ORDER_REVOKED/EXPIRED → MARKET_LIMIT/FEE_CAP/FEE_GE_RECEIVE → CUMULATIVE_QTY_EXCEEDED/INSUFFICIENT_CONFIRMED_BALANCE. 여러 실패가 있으면 이 순서, 동순위는 schema tag순. 외부 응답은 `code`, `retryable` boolean, `state`, 확정 관측 `height`를 포함하며 다른 사용자의 계정 정보는 공개하지 않는다. 저장된 동일 요청 재시도는 wire/context/auth 검증 후 이전 결과를 반환하고 현시점 만료로 성공 영수증을 덮어쓰지 않는다.

## 만료·체결·정수

Order 정산 실행 높이 h < expiry_height. h==expiry부터 EXPIRED. Cancel은 시퀀서 확정 관측 높이를 WAL에 기록하고 같은 등호 적용. Wallet은 issued_at <= now < expiry_time, 0 < TTL <=120, nonce 원자 소비. origin은 lowercase HTTPS scheme/host, 기본 443 포트 생략, 비기본 포트만 명시; path/query/fragment/userinfo 금지, allowlist 정확 일치. 로그인 인증을 거래 권한으로 사용하지 않는다.

U32/U64/U128 범위를 정확히 검사하고 곱·합 중간값 U256 초과도 거절한다. 최종 잔고/금액 U128, q/p/epoch/seq U64. base=q*1000 atoms, quote=q*p atoms. base/quote decimals=6이므로 lot=0.001 BASE, price tick=0.001 QUOTE/BASE. quantity/price 입력은 정확한 배수여야 하며 반올림 금지. 수수료는 fill별 ceil(receive*bps/10000), bps=0이면 0, 양수 수수료>=수취액 거절. cap은 fill별 bps 상한이다. fee profile은 manifest에서 선택한 단 하나의 version이며 같은 높이에서 혼용 금지. 분할 체결은 합산 수수료를 증가시킬 수 있다.

C_start gross debit 선검사, 배치 내 수취액 재사용 금지, 한 fill 실패 시 전체 rollback. C/T/U 및 A=C-R-D는 음수 금지, P는 A에 포함하지 않는다. buy D는 한도가 최악 입력액. 확정 또는 확정 거절+재생 전에 차액 해제 금지. 출금은 signer=owner=수취인, epoch 증가와 차감 원자적. ACK된 주문은 WAL/outbox에서 재생 가능해야 하며 시퀀스 소실을 성공으로 간주하지 않는다. IOC callback은 최종 매칭 명령 순서에서 남은 R만 해제하고 이미 잠정 체결된 D/P는 보존한다. 재생 중 callback 중복도 효과는 1회다. 직접 회수는 운영자 서명 없이 사용자 TX로 가능해야 한다.

## Batch 영수증·재시도

새 배치는 last_seq+1 및 previous hash 일치. 빈 fills 배치는 EMPTY_BATCH로 거절(빈 repeated wire 자체는 canonical). 증빙은 사용된 주문만, order_hash순; fill은 (command_seq,match_index)순, 중복/역순 거절. seq는 operator 교체에도 연속. BatchCore는 tag7만 제외; hash 규칙은 baseline 그대로.

영수증은 (chain_id,genesis_hash,market_id,batch_seq)로 영구 조회하며 BatchReceiptV1의 모든 필드를 보관한다. 확정 C/T·SeenFill·주문누계·영수증을 하나의 원자 commit에 쓴다. **과거 seq도** 저장된 id/hash가 같으면 기존 receipt 반환(ALREADY_COMMITTED), 다르면 BATCH_CONFLICT. 최신 seq 연속성 검사보다 과거 영수증 확인이 먼저다. 재시도 권한은 현재 승인된 운영자 TX; 과거 Batch의 operator_epoch는 이미 확정된 영수증 조회에 재적용하지 않는다. 현재 운영자 권한이 없는 호출도 공개 read API로 영수증을 조회할 수 있지만 새 효과는 만들 수 없다.

GET `/v1/markets/{market}/batches/{seq}?chain_id=...&genesis_hash=...`: COMMITTED+receipt 또는 NOT_FOUND_AT_HEIGHT+observed_height. NOT_FOUND는 거절 증명이 아니다. timeout/disconnect는 SUBMISSION_UNKNOWN(retryable=true); D/P 유지, 영수증 조회 후 동일 bytes/id로만 재시도. REJECTED_FINAL은 해당 TX의 확정 실패 증거를 뜻하며 다른 in-flight 재시도 가능성까지 해소하고 원명령 재생 후 D/P 정정한다. 중복/응답유실 때 새 seq/id로 재발행 금지. receipts/SeenFill/payment tombstone은 S0에서 GC 없음.

이미 확정된 seq인데 receipt가 없으면 RECEIPT_INCONSISTENCY로 정산을 중단하며 신규 이동·D/P 해제를 금지한다. 새 배치의 seq gap=BATCH_SEQUENCE_GAP, prev mismatch=PREVIOUS_BATCH_HASH_MISMATCH, duplicate fill=DUPLICATE_FILL. 이 코드는 retryable=false이며 재조회 후 상위가 재계획한다. REST/WS 상태는 PENDING(잠정), COMMITTED(확정), CORRECTED(확정 거절·재생 완료), SUBMISSION_UNKNOWN(불확실); event는 entity_id, revision U64, observed_height, state를 포함하고 같은 revision 재전송은 효과 1회, 역행은 무시한다. 확정 receipt 자체를 CORRECTED로 변경하지 않는다.

## 송금·후원 테스트 설정

TransferStableV1.sender는 TX signer, recipient는 raw20, DEVQUOTE만 허용. sender가 amount+service fee 부담, recipient는 amount 전액 수취. (sender,payment_id)는 최초 canonical hash에 결합; 동일 본문은 이전 결과, 다른 본문 PAYMENT_ID_CONFLICT. 신규 h>=expiry 거절, 성공 재조회는 만료 후에도 유지. 수수료 정책 버전·서명 max_service_fee_atoms·테스트 송금 한도를 모두 만족해야 한다.

후원 가스 예산은 DEVGAS 합성 atoms이며 유료 자원 구매가 아니다. floor(height/1000) 창으로 owner/global cap을 공유하고 실행 전 예약하여 동시 요청 과소집계를 막는다. 최종 가스 소비는 실패 TX도 차감하고 미소비 예약만 반환; 제출 결과 불명은 예약 유지. 소진 시 SPONSOR_BUDGET_EXCEEDED, 사용자의 추가 가스 차감을 자동 승인하지 않는다. 실제 가스량 적정성·별도 sponsor 장애 회수는 후속 실측 대상.

## 벡터와 구현 인계

`vectors/signatures.json`, `batches.json`, `integers.tsv`, `policy-cases.json`은 M0 Security r1 원본 바이트를 보존한 **m0-crypto 프로필**이다. fields에서 얻은 chain/market/genesis를 fixture context로 사용하며 DEV 설정과 혼합하지 않는다. 암호 긍정은 full business acceptance가 아니다. `vectors/s0-cases.json`은 DEV 설정·receipt/재시도 판단 입력과 기대값이다. Go/Rust/TS 모두 같은 파일을 읽고 독립 인코딩/검증한다. Python 도구는 예상 바이트/해시와 설정 산술의 무결성만 검증한다. C/D/F의 실제 암호 교차검증과 G/H의 독립 QA는 NOT_RUN이다.

runtime manifest는 git SHA, contract/vector/config hash, Go/Rust/Node 및 lock hash, 실제 genesis bytes hash, 활성 fee versions, 환경·실행명령·결과를 모두 채워야 한다. candidate manifest의 null은 실패/미연결 경계이며 runtime 사용 허가가 아니다. genesis 미제공인 DEV profile을 체인 권한 검증에 사용하지 않는다.
