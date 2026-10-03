# 개발 환경과 문서 기여

이 페이지의 기존 설명은 **S1 고정 기준**이다. S2 후보의 두 자산·주문·복구는 [S2 시작 안내](s2-quickstart.md), 적용 SHA와 인수 상태는 [검증 기록](verification.md#s2-통합-후보)을 따른다.

[문서 목차](README.md) · [S1 사용자 설치](quickstart.md)

## 버전과 실행 범위

제품 기준 `32781aa97d62ec747e7a25c10fdb8b58030d79f2`의 저장소 pin을 사용한다. `chain/`과 `chain/app/`은 별도 Go module이다.

| 작업 | 도구 | 원본 |
|---|---|---|
| S1 앱 빌드 | Go 정확히 1.26.5, GOTOOLCHAIN=local | [검사 스크립트](../chain/app/scripts/check-toolchain.sh), [go.mod](../chain/app/go.mod) |
| S1 사용자 시연 | Python ≥3.10, Node ≥22.18, npm, Chrome, macOS/Linux | [시작 안내](quickstart.md), [Wallet](../web/s1/README.md) |
| 저장소 S0 CI 재현 | Go 1.24.4, Rust 1.92.0, Node 24.21.0, Python | [S0 workflow](../.github/workflows/s0.yml), [.node-version](../.node-version), [rust-toolchain](../rust-toolchain.toml) |

SDK v0.55.0, CometBFT v0.40.0은 [S1 계약](../protocol/s1/CONTRACT.md)과 앱 go.mod/go.sum을 함께 따른다. S0 초기 골격의 Rust 1.85.1·Node 22.14.0 설명은 [역사 기록](development.md)이며 현재 pin이 아니다. S1 사용자 입출금 시연에는 Rust/Docker/MetaMask/Paperclip 토큰이 필요하지 않다.

저장소 루트에서:

```sh
# S0 환경에서: 현재 checkout의 구성요소·벡터 검사
make scaffold vectors
# S1 Go 환경에서: 실제 앱 빌드
(cd chain/app && sh scripts/build.sh)
# S1 Wallet 빌드·DIRECT 검증
(cd web && npm ci --ignore-scripts && ./node_modules/.bin/tsc -p s1/tsconfig.json && node --experimental-strip-types --test s1/direct.test.ts && node s1/build.mjs)
# REST 단위 경계 검사: 실제 다중 노드 시험과 구분
python3 -m unittest discover -s settlement/s1 -v
```

S0 독립 보안은 [재현 안내](../security/README.md), 실제 개발망·REST·브라우저 시험은 각 [ops](../ops/s1/README.md), [REST](../settlement/s1/README.md), [Wallet](../web/s1/README.md)의 전용 명령을 사용한다. 새 home과 전용 포트를 사용하고 기존 사용자 원장에 fixture 시험을 실행하지 않는다. 변경 영향에 맞는 작은 검증을 선택하되 저장소 필수 CI는 생략하지 않는다.

## 변경과 검토

1. 실제 remote main SHA를 확인하고 다른 작업을 보존하는 이슈 브랜치/worktree를 만든다.
2. 동작 변경을 해당 사용자·개발·운영 페이지에 반영한다. 새 기능은 구현·검증 여부와 SHA를 적고 규약/schema를 복제하지 않는다.
3. PR 본문에 **문서 영향**을 기록한다: 수정한 페이지, API/단위/버전/설정 변화, 실행한 명령·환경·결과, 상속한 증거, 남은 제약. 갱신이 불필요하면 이유를 적는다.
4. 링크·앵커·예제를 확인하고 [변경 기록](CHANGELOG.md)에 사용자 영향을 남긴다. 최초 실패·원시 증거·검토 결정을 덮어쓰지 않는다.
5. 후보 head·필수 CI와 함께 CTO → QA 네이티브 검토를 요청한다. QA는 안내를 독립 재현한다. 리뷰어는 판정하고, 실행 담당자가 승인된 head를 정상 병합한다. main SHA·CI·최종 접근 경로를 확인하기 전 완료라고 보고하지 않는다.

키·seed·토큰·node home/DB는 문서나 증거 ZIP에 넣지 않는다. 공개키 JSON은 백업이라고 설명하지 않는다. shared runtime 변경·제품 구현·공개 배포는 별도 승인 범위다.

## S2 문서·통합 검토

S2는 Go 1.26.5·Rust 1.92.0·Node 24.21.0과 기존 lock으로 같은 checkout을 빌드한다. [S2 CI](../.github/workflows/s2-integration.yml)와 [운영 검증 명령](../ops/s2/README.md)을 따른다. Docs의 CTO→QA 검토 뒤 main 통합 실행은 [CEO의 NUS-44](/NUS/issues/NUS-44), 새 main checkout 독립 QA는 [NUS-45](/NUS/issues/NUS-45)가 맡는다. 문서 PR만으로 main 전달 완료를 선언하지 않는다.
