# protocol/v1 · S0-A

검토 후보 v1.0.0-rc2. **독립 최종 검증 전**. CONTRACT.md가 규범, protocol.proto/schema.json이 tag/type, dev-config.json이 합성 설정이다. m0-baseline.md는 원본 이력이며 후보 문구는 CONTRACT의 결정으로 대체한다.

저장소 루트에서 `python3 protocol/v1/tools/check.py`. 추가 패키지/서비스/실자산 불필요. Python 3.9+ 기준. 이 검사는 보존 벡터 byte/hash, 계약 파일 무결성, 테스트 설정 산술·판정 및 wire shape 예제만 검증한다. proto compiler·Go/Rust/TS 제품 parser/ML-DSA 교차검증·runtime CI는 NOT_RUN.

Go: fields의 u32/u64를 엄격 decimal parse, atoms math/big 범위 검사. Rust: u32/u64/u128 checked 및 U256 범위 검사. TS: 전 정수 BigInt, Number 경유 금지. 세 구현 모두 signatures positives의 fields→canonical_hex→sign_input_hex→sha256, 고정 signature 검증, negatives 거절을 독립 실행한다. batches의 candidate는 positive wire라고 가정하지 않는다. policy와 s0 cases의 expected는 제품 검증기로 대조한다. crypto-only와 market/state acceptance 결과를 분리한다.

manifest.candidate.json은 파일별/집합 해시와 미연결 runtime 필드를 포함한다. 집합 hash=정렬 경로별 `sha256 + 두 공백 + relative_path + LF`의 UTF-8 SHA256. contract는 vectors/evidence 제외 문서·schema·config·tool 파일; vector는 vectors 전체. self hash 순환을 피하려고 manifest 자체는 제외한다.

M0 fixture의 공개 모의 키만 사용한다. 서명 원본을 DEV market으로 바꾸면 서명은 무효이므로 profile을 섞지 않는다.

SEC-A-01/02: `vectors/amount-codec.json`과 `vectors/message-codec.json`을 세 언어가 함께 소비한다. API atoms는 정규 십진 문자열, wire는 16-byte big-endian이다. message positives의 완전한 API JSON→canonical_hex를 독립 계산하고 송금은 payment_frame_hex/payment_hash도 대조한다. wire_cases와 state_cases의 기대 판정은 각각 parser 및 상태 어댑터로 검증한다. `tools/check-message-codec.py`는 이 기대값의 독립 Python 대조이며 제품 인수가 아니다.
