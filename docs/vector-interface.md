# C~F runner 연결점 — SRE 배선 제안

공통 protocol 정의는 CTO 소유다. 이 문서는 기존 M0 배선의 인계이며 공통 schema 확정이 아니다.
`ops/ci/manifest.json`의 contract_revision, vectors, vectors_sha256 및 각 lane.vectors는 현재 null이다.
C~F 입력 전 `make vectors`는 BLOCKED(exit 2). sha는 합의된 실제 파일 bytes의 SHA-256이다.

| 구현 | cwd | build/test | 후속 vector argv 예시 (현재 존재하지 않음) |
|---|---|---|---|
| Go C | chain | go build/test ./... | go run ./cmd/vectors {vectors} |
| Rust D | exchange | cargo build/test --locked | cargo run --locked --bin vectors -- {vectors} |
| TS F | web | npm run build / npm test | node dist/vectors.js {vectors} |
| E | settlement | 담당자 계약 시험 | receipt/API fixture를 CTO manifest에 참조 |

명령은 shell이 아닌 argv 배열로 실행하고 {vectors}만 절대 파일 경로로 치환한다.
stdout은 단일 JSON, 로그는 stderr, timeout 600초다. 각 언어는 기대값·필수 case ID 집합을
자체 assertion으로 검증하고 불일치 시 nonzero로 종료해야 한다.

```json
{"contract_revision":"agreed-revision","vectors_sha256":"sha256","results":[{"id":"case-id","sign_bytes_hex":"00","valid":false}]}
```

비교기는 revision/hash/nonempty results와 세 언어 결과 완전 일치를 검사한다.
세 구현이 같은 오답 또는 같은 부분 집합을 출력하는 경우 단순 일치로는 탐지하지 못한다.
따라서 G/H는 기대값/필수 ID 누락을 독립 검증해야 한다. S0-A에서 영수증 schema가 달라지면 CTO와
manifest 및 비교기를 함께 바꾸고 회귀 시험한다. contract/vector hash를 임의 값으로 채우지 않는다.
