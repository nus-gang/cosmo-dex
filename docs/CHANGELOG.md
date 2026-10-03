# 변경 기록

## S2-G 문서 후보 — 통합 기준 84ea152

### Added

- [S2 시작 안내](s2-quickstart.md)에 두 자산 실제 예치·GTC/부분 체결·잔량 취소·제한 IOC, 출금 보류와 같은 home 재시작 절차를 추가했다.

### Fixed

- [PR #35](https://github.com/nus-gang/cosmo-dex/pull/35)의 승인·증거·인계 링크에 Paperclip origin을 명시해 GitHub에서도 로컬 보드로 연결되게 수정했다. 다른 설치의 주소 적용은 [검증 안내](verification.md)를 따른다.

- README·목차·공통 안내의 S1 범위를 명시하고 S2 후보의 기능·API·운영 및 [검증 경계](verification.md#s2-통합-후보)를 연결했다.
- 공개키 JSON이 백업이 아님을 유지하고 로컬 접수/잠정 체결과 체인 확정·main 전달 상태를 구분했다.

[문서 검토·PR](http://localhost:3100/NUS/issues/NUS-42), [기능 후보 PR #34](https://github.com/nus-gang/cosmo-dex/pull/34). 기존 실패/제외 증거를 보존하며 제품 코드·규약·QA 판정은 변경하지 않는다.

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
