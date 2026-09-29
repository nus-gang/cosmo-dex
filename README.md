# cosmo-dex S0

Go/Rust/TypeScript 모노레포 빌드 골격. 실제 체인·거래·지갑 구현은 미연결이다.

`make scaffold`는 골격 빌드와 CI 제어 흐름을 검증한다.
`make vectors`는 실제 공통 manifest/runner가 없으면 exit 2로 실패한다.

[개발 및 인계 문서](docs/development.md) · [runner 연결점](docs/vector-interface.md)
