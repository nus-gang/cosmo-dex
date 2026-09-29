# cosmo-dex S0

Go/Rust/TypeScript 공통 rc4 계약, 정산·API 모의 계약, 공통 CI와 독립 보안·QA 검증 자료를 포함한다. 실제 체인·원장·REST/WS 연결과 제품 T01~T16은 아직 미완료다.

`make scaffold vectors`는 현재 checkout의 구성요소와 고정된 공통 벡터를 검사한다.
`bash security/run.sh`는 현재 checkout의 Go/Rust/TS 구현으로 독립 교차 검증을 다시 실행한다.

[개발 및 인계 문서](docs/development.md) · [runner 연결점](docs/vector-interface.md)

[S0 main 통합 및 재현](docs/s0-main-integration.md) · [보안 재현](security/README.md)
