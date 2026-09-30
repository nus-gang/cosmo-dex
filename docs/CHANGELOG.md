# 변경 기록

## DOC-1 문서 후보 — 제품 기준 32781aa9

### Added

- [사용자 시작 안내](quickstart.md)에서 설치·두 계정 100→40→60·종료·키 유실 시 다음 세션을 재현할 수 있도록 기존 승인 안내를 저장소에 연결했다.
- [구성](architecture.md), [API 탐색](api.md), [기여 지침](contributing.md), [검증 근거](verification.md), [인벤토리](inventory.md)를 추가했다.

### Fixed

- [README](../README.md)·[목차](README.md)의 S0 미연결 설명을 실제 S1 로컬 입출금 기준으로 갱신했다.
- 초기 골격·REST·Wallet의 당시 후보 상태와 현재 main 인수를 구분하고, 역사적 실패·원본 증거는 보존했다.

제품 코드·규약·QA 판정은 변경하지 않는다. 문서 후보 PR과 최종 main 전달은 [DOC-1](http://localhost:3100/NUS/issues/NUS-34)에 기록한다.

## S1 인수 — 32781aa9 (2026-09-30)

- 실제 로컬 4검증인·REST·브라우저 테스트 자산 입출금과 확정 조회를 연결했다.
- 오래된 잔고 응답 및 금액 안내를 수정했다. 최초 FAIL·수정 근거·검증 범위는 [검증 기록](verification.md)을 따른다.

## S0 인수 기록

- 공통 rc4 계약·구현·CI·독립 보안/QA를 통합했다. 원본 PR과 SHA는 [S0 통합 기록](s0-main-integration.md)에 보존한다.
