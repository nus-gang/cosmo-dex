# S0-B 개발 및 인계

> 이 페이지는 S0-B 초기 골격의 역사 기록이다. 당시 미구현·버전·runtime 설명을 현재 상태로 해석하지 않는다. 현재 개발 pin·기여 절차는 [기여 지침](contributing.md), S1 실행은 [사용자 시작 안내](quickstart.md)를 따른다.

승인 범위: S0-A~H, 모의 키·합성 데이터. 유료 증설·운영 배포 없음.

## 책임과 통합

| 경로 | 소유자 | 변경/리뷰 |
|---|---|---|
| chain/ | Chain | Go codec 및 체인 연결, CTO/Security |
| exchange/ | Exchange | Rust codec 및 엔진, CTO/Security |
| settlement/ | Settlement | 영수증/API 계약, CTO |
| web/ | Wallet | TS/브라우저/지갑, CTO/Security |
| protocol/ | CTO | 공통 schema/설정/벡터, Security/QA 검증 |
| ops/, .github/workflows/ | SRE | 빌드·실행·복구, CTO |
| tests/ | QA/Security | 공통 manifest 교차 검증 및 인수 |
| docs/ | 해당 담당자 | CTO 통합 |

빈 패키지 빌드·ESM 로딩은 제품 시험이 아니다. Go/Rust 제품 test는 아직 0건이다.
Go는 외부 의존성이 없어 go.sum을 만들지 않는다. 추가 시 go.mod/go.sum을 함께 제출한다.
Rust Cargo.lock, TS package-lock.json을 커밋한다. settlement는 담당 언어 결정 전 문서 경계만 둔다.

## 독립 checkout과 재현

Paperclip 프로젝트 workspace `61a53d01-c9ea-45dd-b274-eb954095ee6a`에 저장소 URL과 main ref를 등록했다.
이번 실행의 기존 workspace를 강제로 바꾸지 않고 `worktrees/NUS-11`에 독립 clone을 만들었다.
브랜치 `sre/nus-11-s0-build`, 원본 main `7942e93`. 공유 M0 경로는 읽기만 했다.
새 담당자는 자신의 workspace 아래 독립 clone과 이슈 브랜치를 사용한다.

```sh
git clone https://github.com/nus-gang/cosmo-dex.git worktrees/NUS-XX
cd worktrees/NUS-XX
git switch -c owner/nus-xx-topic
# Go 1.24.4, Rust 1.85.1, Node 22.14.0; Python 3, make
rustup toolchain install 1.85.1 --profile minimal --component rustfmt --component clippy
make scaffold
make vectors
```

`make scaffold`: npm ci → Go/Rust/TS 빌드 → 언어 test → CI 실패 경계 시험.
`make vectors`: manifest 입력 부족 시 Python exit 2 (make 자체 exit도 2), 오류 exit 1; 우회 성공 없음.
고정 버전은 골격 재현 기준이며 Cosmos/서명 라이브러리 호환성 또는 운영 보안 인증이 아니다.
Chain SDK가 요구하는 Go 버전 등은 C~F 통합 PR에서 CTO와 lock을 함께 갱신한다.

## CI 판정

`Go Rust TS scaffold` job은 골격 인수 증거다. `Full vectors (requires C-F runners)`는
매 push/PR에서 별도 job으로 실행하며 미구현 runner/manifest는 실패한다. 전체 workflow가 빨간 상태면
제품 통합 통과로 보고하거나 merge하지 않는다. 초기 B는 골격 job과 실제 실패 경계 증거를 CTO가 검토한다.
`continue-on-error`, fallback runner, 비어 있는 벡터 성공은 사용하지 않는다.
Actions는 조회한 commit SHA에 고정했고 permissions=contents:read, persist-credentials=false,
실행 15분 제한, 동일 ref 실행 취소를 둔다. hosted ubuntu-24.04 이미지는 가변이므로 bitwise 재현 보장은 없다.
Evidence artifact에 checkout SHA, toolchain, lock/manifest hash, 빌드 로그, vectors 판정을 저장한다.

각 담당자 PR → 해당 담당 리뷰 → CTO 네이티브 review → 저장소 정책에 따른 main 통합 순서다.
B를 C~F의 blocker로 추가하지 않는다. C~F가 실제 runner를 제공하면 같은 manifest에 연결하고,
G가 독립 보안 검증, H가 새 checkout 재현을 판정한다. 필요한 리뷰/CI 없이 자동 merge하지 않는다.

## Paperclip 관리 개발망

현재 실제 binary/genesis/4검증인 프로세스와 execution runtime은 미연결이다. 이 골격은 서버가 아니다.
M0 `ops/topology-mock.json`은 배치 참고용이며 유효한 genesis/키가 아니다.
Chain이 실제 앱 CLI·binary hash·같은 genesis·서명 상태 경로를 제공하면 후속 승인 범위에서
Paperclip execution workspace에 foreground supervisor command를 등록한다.

```sh
# 관리 workspace와 명시적 command가 준비된 경우만 사용
export SRE_WORKSPACE_COMMAND_ID='<configured-four-validator-command-id>'
bash ops/runtime.sh start
bash ops/runtime.sh stop
```

이 스크립트는 heartbeat-context에서 currentExecutionWorkspace를 읽고 지정 command만 관리 API로 실행한다.
workspace 부재는 exit 2, command 부재/HTTP 오류는 실패다. 비관리 백그라운드 실행 fallback은 없다.
실제 시작 후 runtime_service work product에 반환된 ID/URL/readiness를 등록한다.
현재 runtime_service URL을 생성하거나 4검증인 성공을 주장하지 않는다.
기동 인수는 동일 genesis/app hash, 블록 증가, 1대 손실 진행/2대 손실 정지, 복귀 수렴이다.
키·백업·WAL ACK/fencing·비상 RPC/가스·관측은 [M0 운영 설계](../ops/OPERATIONS-M0.md)를 따른다.
로컬 재시작은 분산 장애 또는 서비스 RPO/RTO 근거가 아니다.

## 공식 버전 근거

- [Go go1.24.4](https://github.com/golang/go/tree/go1.24.4), tag object 6796ebb2cb66b316a07998cdcd69b1c486b8579e
- [Rust 1.85.1](https://github.com/rust-lang/rust/tree/1.85.1), tag object 0035dbbca59afecf0b4e53d96d61e00680dca9de
- [Node 22.14.0](https://nodejs.org/en/blog/release/v22.14.0)
- [TypeScript 5.8](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-8.html), npm lock 5.8.3 integrity 확인

실제 조회일 2026-09-29. Actions v4의 실제 ref SHA를 GitHub API로 조회하여 workflow에 고정했다.
