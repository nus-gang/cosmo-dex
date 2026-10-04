# API·서명·금액 안내

이 페이지의 기존 설명은 **S1 고정 기준**이다. S2의 두 자산·주문·복구는 [S2 시작 안내](s2-quickstart.md), 적용 SHA와 인수 상태는 [검증 기록](verification.md#s2-main-인수)을 따른다.

[문서 목차](README.md) · [사용자 시작 안내](quickstart.md)

기준: `32781aa97d62ec747e7a25c10fdb8b58030d79f2`. 이 페이지는 탐색 안내다. 필드·직렬화·허용값의 권위는 [S1 계약](../protocol/s1/CONTRACT.md), [메시지 schema](../protocol/s1/messages.proto), [REST 구현](../settlement/s1/server.py)이다. S0 [protocol/v1](../protocol/v1/README.md)의 strict wire/ORDER frame 규칙을 S1 SDK envelope에 적용하지 않는다.

## 조회와 제출

[REST 상세](../settlement/s1/README.md)의 경로를 사용한다. 아래 조회는 사용자 시작 안내의 API가 실행 중일 때 가능하다. `OWNER`는 화면의 실제 nus 주소, `TX_HASH`는 원래 TX의 uppercase SHA256으로 바꾼다.

```sh
curl --fail http://127.0.0.1:8787/s1/network
OWNER='<화면의 nus 주소>'
curl --fail "http://127.0.0.1:8787/s1/accounts/$OWNER"
TX_HASH='<원래 TX의 uppercase SHA256>'
curl --fail "http://127.0.0.1:8787/s1/txs/$TX_HASH"
```

| 경로 | 의미 |
|---|---|
| `GET /s1/network` | chain/genesis·자산 정밀도·committed 높이 |
| `GET /s1/accounts/{owner}` | 은행·거래소 확정 잔고·가스·sequence·epoch |
| `POST /s1/txs` | `{"tx_bytes":"canonical padded base64"}` 제출, 202 UNKNOWN |
| `GET /s1/txs/{uppercase SHA256}` | 실제 블록 포함·TX bytes/hash·실행 결과 대조 |
| `GET /s1/accounts/{owner}/requests/{lowercase hex64}` | 같은 committed snapshot 높이의 immutable receipt |

금액과 높이 등 정수 필드는 십진 문자열이다. HTTP 성공과 CheckTx 0은 확정이 아니다. COMMITTED는 실제 블록 포함과 실행 code 0, REJECTED_FINAL은 포함됐으나 실행 code 비0이다. index 지연·미존재는 UNKNOWN이며 `NOT_FOUND_AT_HEIGHT`는 관측 높이의 부재일 뿐 최종 실패가 아니다. 결과 불명확 시 기존 hash/request ID/서명 bytes로 확인하고 새 sequence·ID로 자동 재서명하지 않는다.

계정 조회는 committed snapshot을 사용한다. REST의 `observed_height`, `block_time`, `freshness_ms`, `query_latency_ms`와 브라우저의 계정/network 높이를 함께 본다. 과거 높이 응답·역순 응답은 현행 Wallet의 신선도 검사를 거치며 잔고가 숨겨질 수 있다. 정지한 체인의 과거 COMMITTED 잔고는 새 확정을 의미하지 않는다.

## 서명과 단위

[Wallet DIRECT 구현](../web/s1/direct.ts)과 [Chain 앱 경계](../chain/app/README.md)를 참조한다. SDK `SIGN_MODE_DIRECT=1`의 protobuf SignDoc 그대로를 ML-DSA-65로 서명하며 TxRaw SHA256이 TX hash다. owner·등록 공개키·chain ID·실제 genesis·account_number·sequence·fee를 검증한다. signer/message/signature 각 1개이며 출금 recipient는 owner 고정이다. 공개키 입력 형식은 [USER-PUBLIC-KEYS.md](../chain/app/USER-PUBLIC-KEYS.md)가 권위다.

| 화면 DEVQUOTE | CLI/API amount_atoms |
|---|---|
| 100 | `100000000` |
| 40 | `40000000` |
| 60 | `60000000` |

`decimals=6`, 정수 연산을 사용한다. DEVGAS는 DEVQUOTE와 섞지 않는다. 화면 기본 수수료는 **1000 DEVGAS atoms**, gas limit **500000**이며 실패에도 ante 단계의 가스·sequence 소비가 가능하다. 메시지 rollback과 별개다.

[CLI 예시](../ops/s1/README.md#실제-거래)의 `tx --user`와 `receipt --user`는 공개 fixture 키 전용이다. 브라우저 공개키로 초기화한 개발망에는 이 명령으로 사용자 서명을 대신하지 않는다.

## S2 탐색·인증·상태

S2 API는 loopback 8788이다. [공통 계약](../protocol/s2/CONTRACT.md)·[schema](../protocol/s2/schema.json)·[profile](../protocol/s2/profile.json)·[REST 구현](../settlement/s2/server.py)이 필드와 오류의 원본이다. 실행 중인 S2에서 공개 상태를 확인한다.

```sh
curl --fail http://127.0.0.1:8788/s2/network
curl --fail http://127.0.0.1:8788/s2/status
curl --fail http://127.0.0.1:8788/s2/book
```

`/s2/auth/challenges`와 `/s2/auth/sessions`는 WalletChallenge의 origin·nonce·TTL을 검사한다. `/s2/orders`, `/s2/cancels`, `/s2/me`와 개인 명령 receipt에는 해당 owner 세션이 필요하다. 공개 호가와 개인 조회를 혼용하지 않는다. 토큰·개인키를 문서나 로그에 복사하지 않는다.

명령 `LOCAL_ACCEPTED`는 `LOCAL_FSYNC`·`replicated=false`인 로컬 접수이며 체인 TX의 `COMMITTED`와 다르다. UNKNOWN 주문은 receipt 및 같은 서명 원문/ID로 확인한다. DIRECT `/s1/txs`는 동일 S2 API의 별도 체인 제출 경로이며 개인 주문 세션을 직접 출금 권한으로 사용하지 않는다. S2 owner는 base64 20-byte, S1 account/receipt 경로 owner는 `nus` bech32다. [직접 조회 경계](../settlement/s2/README.md#receipt응답-유실부분-체결-ioc-추가-검증)를 따른다.
