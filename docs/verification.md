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
