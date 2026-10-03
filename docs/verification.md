# 적용 버전과 검증 근거

[문서 목차](README.md) · [문서 인벤토리](inventory.md)

## 제품 기준과 문서 전달 상태

제품 기준 main은 `32781aa97d62ec747e7a25c10fdb8b58030d79f2`, tree `a053dfa8885d489791cf992fb938c6fc8880cf4f`다. DOC-1 시작 시 `git ls-remote origin refs/heads/main`으로 일치함을 확인했다. quickstart는 재현을 위해 이 제품 SHA를 checkout한다. 문서 후보의 CTO·QA 승인 및 main 전달은 별도이며 [DOC-1 작업](http://localhost:3100/NUS/issues/NUS-34)에서 후보 head·실행 근거·최종 main SHA를 확인한다.

아래 Paperclip 링크는 **Paperclip 서버 기준 경로**다. `http://localhost:3100`은 이 프로젝트의 로컬 보드 주소이며, 다른 설치에서는 자신의 Paperclip origin으로 바꾼다. 저장소의 공개 URL이나 배포된 제품 URL이 아니다. 접근 권한이 있어야 한다.

## 기존 인수 증거

아래 판정은 해당 SHA의 기존 증거를 인용한 것이며 DOC-1에서 새로 전체 시험을 실행했다는 뜻이 아니다.

- [S1 최종 인수](http://localhost:3100/NUS/issues/NUS-1#document-s1-completion): S1-AT01~07 PASS, main CI 8 workflows·9 jobs 성공. 공유 runtime의 최종 SHA 갱신은 미확인.
- [독립 QA 판정](http://localhost:3100/NUS/issues/NUS-24#document-qa-verdict-32781aa9): revision `f41c1d56-f785-4c63-9485-28ff35e58c4f`.
- [독립 QA 실행 안내](http://localhost:3100/NUS/issues/NUS-24#document-qa-guide-32781aa9): revision `f8703eb8-e842-4d73-ac6c-cf7111bb147b`.
- [사용자 안내 원본](http://localhost:3100/NUS/issues/NUS-1#document-s1-local-user-guide): revision `ed7d05e0-87d2-4b6e-842b-200952573457`, [안내 파일](http://localhost:3100/api/attachments/c4b65c74-4783-4a84-a186-cd01ba7f5536/content?download=1).
- [S1 인수·안내 ZIP](http://localhost:3100/api/attachments/f866efc0-72b1-42b7-b049-3e63aa1284de/content?download=1), SHA256 `0c2bb412d508dbef3b7c5bc33bd2d38d9eed5da12284f32eecf5b8fca1b6eca4`.
- [QA 원시 보고서·화면·TX](http://localhost:3100/api/attachments/ceb44345-f76f-4336-a99b-2ac9a075657f/content?download=1), [수정 통합·CI](http://localhost:3100/NUS/issues/NUS-1#document-s1-fix-integration).

S0 969/969 비교(872 전체 응답·97 지정 필드), rc4 60×3, 브라우저 453 checks는 S0 회귀다. optional 앱 CLI 단위 1개 SKIP을 보존한다. 원래 [T01~T16 추적](http://localhost:3100/NUS/issues/NUS-24#document-qa-trace-32781aa9)은 **전체 PASS 0/16, 부분/NOT_RUN**이다. S1 합격을 제품 전체 시험 합격으로 환산하지 않는다.

## 최초 실패와 수정

최초 main `913da14`의 QA FAIL·보류는 삭제하지 않는다. QA-S1-01/B2의 과거 잔고 역행은 stale/역순 응답·계정/세션/genesis 전환 검사와 잔고 숨김으로 수정됐다. QA-S1-02/B3는 atoms 안내 정정 및 CLI 시연으로 해소됐다. 상세 판정·원시 파일은 위 QA와 수정 통합 링크에 남는다. REST의 최초 계정 배열 순서 가정에 따른 receipt 404와 수정 이력은 [REST 안내](../settlement/s1/README.md#4검증인-rest-검증)에 보존한다.

## 설계 원본과 현재 기능의 구분

- [아키텍처 PDF 원본](http://localhost:3100/api/attachments/d157936f-f4b3-45e4-849a-f68621a7b90d/content)
- [개발 설계 PDF 원본](http://localhost:3100/api/attachments/9c378275-dcec-4402-8478-4bf7d26af911/content)
- [M0 운영 설계](../ops/OPERATIONS-M0.md), [S0 계약 원본 근거](../protocol/v1/evidence/SOURCES.md)

PDF의 장기 설계·성능 목표는 현재 구현 증거가 아니다. 주문·매칭·체결 정산·영속 키 복구·독립 비상 회수·외부 자산·실자산 운영은 S1 범위 밖이다. 사용자 안내와 현재 구성은 코드·S1 인수 증거를 기준으로 작성했으며 PDF 전체 재검토를 새 검증으로 주장하지 않는다.

## S2 통합 후보

2026-10-03 23시 UTC 이후 Docs 착수 조회의 원격 main은 `ec8961c9919370bb69afab6463bdac8fcfcab7fd`다. 공유 루트의 초기 HEAD를 기준으로 사용하지 않았다. 문서 대상은 [NUS-41 CTO 승인](/NUS/issues/NUS-41#comment-1e00a129-087b-4cb6-b36d-504f36fae7c0)의 [PR #34](https://github.com/nus-gang/cosmo-dex/pull/34), head `84ea15219512b84cdf0f57c59913ae99ed1f3fee`, tree `eb436444d338cab5482bda29ad4333a981538b22`다. Docs와 Security는 같은 후보를 검토한다.

- 계약 집합 hash: `2e103517c344f21c2b97fbe7e977f0e614c32c704b0f54978c5bb1fb3ae6ab0f`; profile hash: `70281595d471947a56d9bf8a97553dd388a85b107c215a95cc6f34ea9f5f321f`. 집합 계산법과 개별 파일 hash는 [protocol manifest](../protocol/s2/manifest.json)에 있다. manifest의 초기 pending 상태는 계약 작성 당시 기록이다.
- 실제 genesis는 실행별 생성 bytes의 SHA256이다. 고정 fixture hash를 사용하지 않는다. binary·lock·genesis·도구 버전·원시 결과는 [SRE 통합 결과](/NUS/issues/NUS-41#document-integration-results)의 manifest와 [공개 증거 묶음](/api/attachments/caeffb1e-0da5-40f4-9f9b-c92e7852a8da/content)에서 확인한다.
- **상속 증거:** [Linux 통합 CI](https://github.com/nus-gang/cosmo-dex/actions/runs/37159989867)의 실제 4검증인 브라우저 20단계, 종료 모형 5개, CI 실패 전달 모형 12개, Engine 프로세스 6개, Wallet S1/S2 29개, 로그 8개 및 S0/S1 회귀 PASS. CTO는 원격 26/26 성공·원시210개/소스8개 hash를 대조했다. Docs가 이 전체 제품 시험을 새로 실행했다는 뜻은 아니다.
- 기존 lifecycle의 최초 WAL prefix는 비어 있었다. 실제 주문 보존 근거는 브라우저 재시작과 Engine 회귀다. Engine crash 시험은 합성 snapshot 기반이며 체인 모든 장애 조합 또는 분산 내구성의 증거가 아니다.
- **보존한 실패:** CTO-S2F-01은 무응답 검증인 종료에서 잔존 후손/잘못된 exit 0, CTO-S2F-02는 tee 파이프라인의 실패 은폐였다. 수정·재심사·최초 포트 충돌·오염 가능성으로 제외한 로컬 lifecycle 결과는 위 SRE 문서와 [CTO 심사](/NUS/issues/NUS-41#document-cto-review)에 남긴다.
- 상시 preview는 미등록·미검증이고 공유 runtime 최종 SHA도 확인되지 않았다. 로컬 `LOCAL_FSYNC`, 합성 자산, 신뢰 RPC, 탭 키 수명 한계를 유지한다. 기존 T01~T16 full PASS **0/16**, 부분/NOT_RUN을 S2 부분시험으로 올리지 않는다.

문서 자체 검증과 고정 PR/head·CI 결과는 [NUS-42](/NUS/issues/NUS-42)에 게시한다. CTO→QA의 안내 독립 재현 뒤 [CEO main 통합](/NUS/issues/NUS-44)과 [새 main QA](/NUS/issues/NUS-45)가 남는다. PR·구성요소 PASS는 전체 S2 완료나 main 전달 완료가 아니다.
