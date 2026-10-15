# 공개 계정 receipt 심사 패키지

본 디렉터리는 CTO-70-03의 **비활성 계약 후보**다. 제품 코드·활성 protocol·lock 변경0.
이름 `s3-dev-local-account/1`은 Security→QA가 같은 후보를 승인하기 전에는 미승인이다.

- CONTRACT.md: 규범·source tuple·privacy·정수/cap·두 검증자의 책임·호환성.
- API.md: 변경 전후·정확한 인증 조회 경로·오류·capability pin.
- schema.json: 추가/누락 key를 거절하는 공개 envelope.
- reference.py: byte/hash/projection oracle. 서비스·auth/crypto·WAL semantic replay 구현이 아니다.
- vectors/: 원 source frame 및 공개 canonical bytes, 고정 SHA256·기대 오류.
- acceptance.json: C/D/E/SRE/QA의 실제 제품 시험14개. 전부 NOT_RUN.
- MANIFEST.json: 후보 파일·상속204개와 승인 A8개+rc3 manifest의 exact hash; 자기 제외.

검산(설치·네트워크 불필요, Python3.14.0 작성자 실행):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 proposals/s3-local-account-receipt-v1/verify.py
```

read-only 검사다. 생성기/manifest 재봉인으로 mismatch를 고치지 않는다.
제품 시험을 실행하지 않는 만큼 auth 만료·ML-DSA·실제 fsync/marker·same-home restart의
성공을 이 명령으로 주장하지 않는다. oracle의2회 재계산은 저장 bytes 재로딩일 뿐이다.

긍정12개 중4개는 C `47204ab` CTO 회귀의 원 S3D1 frame을 그대로 추출한
fee0/25 maker·taker 기록이다. 파일명 maker-partial의 첫 주문은 이후 부분 체결될
maker의 최초 예약 receipt이며 해당 명령 자체에서 체결됐다는 뜻이 아니다.
원 WAL SHA/record index/출처는 vectors/index.json에 보존했다. 원 전체 prefix·object·
marker·키를 이 패키지에서 복구하거나 실행하지 않는다.
2개 correction은 승인 rc3의 schema-complete 합성 hash fixture다. 나머지6개는
취소/거절/출금준비·abort/복수maker/정산에 대한 합성 projection fixture이며,
경제 재생 또는 서명·체인 proof가 유효하다는 주장이 아니다.

벡터의 full source frame은 **검토 전용 trusted 자료**다. 공개 endpoint에서 내려주지 않는다.
모의 계정·합성 자산의 public 서명/기록만 포함하고 private key·session token은 없다.
schema-valid한 source hash 거짓말을 첫 공개 client가 독립 검출할 수 없다는 제한도
검사한다. trusted verifier와 재조회 ledger가 다른 책임을 갖는다는 의미다.

과거 승인 A·rc3 파일은 그대로이고 이번 overlay는 새 runtime contract hash에 포함돼야 한다.
새 Context/genesis/home/시험 키로 소비하며 old home 또는 축약 응답 자동 migration0.
main·runtime pin·실서비스·G00 지원·durable ACK 승인은 이 패키지의 산출물이 아니다.
