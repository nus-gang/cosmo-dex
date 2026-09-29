# Wallet S0 — 키 수명주기와 UI 재사용 ADR 입력

2026-09-29 · Wallet · CTO→Security 네이티브 검토 제출. 결정권자는 CTO이며 아래 제품 UI 선택은 제안이다.

## 키 생성·백업·복구

S0 구현은 브라우저 CSPRNG 32-byte seed→ML-DSA-65 키 생성, 세션 메모리 서명, 별도 모의 키의 동일 seed 재생성 복구 검증까지다. Node/실제 Chrome에서 성공을 확인했다. 새 seed·secretKey는 외부로 내보내지 않는다. 페이지 종료/키 교체 시 소거를 시도하지만 JS GC·라이브러리 복사본 완전 소거는 보장하지 않는다. 실제 저장 백업·비밀번호 KDF·암호화 파일·복구 UX는 설계만 작성했으며 구현 완료로 표시하지 않는다.

후속 승인 범위에서의 설계 제안:

1. 전용 signer worker가 seed/개인키를 소유하고 UI는 공개키와 제한된 서명 요청만 사용한다. frame 종류·chain/genesis/시장·금액·만료를 사용자 확인 화면에 고정한다. 서명 취소는 요청을 보내지 않는다. origin 인증과 거래 권한을 분리한다.
2. 백업은 버전/알고리즘/공개키 fingerprint를 포함하는 AEAD 암호화 파일로 설계한다. KDF 알고리즘·비용·salt·nonce·복구 비밀번호 정책은 단말 성능과 Security 검토 후 고정한다. 자체 무암호 seed 다운로드·서버 escrow·localStorage 평문 저장은 제공하지 않는다. 구현 전에 형식·오류·잘못된 비밀번호·변조·KDF 자원 제한 시험 벡터를 합의한다.
3. 새 환경에서 백업 복호화→공개키/fingerprint 재생성→확정 계정 key type/raw bytes 조회→owner 비교→일회성 서명 확인 순으로 복구한다. 미등록 키는 등록 TX 확정 전 거래 불가다. 키 교체는 epoch 무효화와 기존 주문 정리 경로를 요구한다.
4. 잘못된 비밀번호/손상 파일은 계정 변경 없이 거절한다. 복구 문구·비밀번호·개인키·서명 요청 원문을 telemetry/에러 로그에 넣지 않는다. worker 격리만으로 XSS·악성 의존성·클립보드·확장 프로그램 위험이 해결된다고 주장하지 않는다. CSP, 의존성 고정, 사용자 확인 변조 방지, 세션 잠금·분실/직접 회수 절차를 Security와 검토한다.

## Helix 출처와 제한

공식 저장소의 아래 고정 소스 원문을 2026-09-29 다시 확인하여 `evidence/upstream/`에 보관했다. 라이선스 적합성의 최종 법적 판정은 아니며 전체 전이 그래프를 검사한 결과도 아니다.

|출처|확인된 사실|조치|
|---|---|---|
|[Helix v0.1.10 리비전 LICENSE](https://github.com/InjectiveLabs/injective-helix-demo/blob/a7c237216547e5bf2468ed30b979c399a456962b/LICENSE)|MIT 원문|이 오래된 태그를 새 코드의 라이선스 근거로 쓰지 않는다|
|[Helix 후보 package](https://github.com/InjectiveLabs/injective-helix-demo/blob/d916c8b20d7a269ced9fe6c7100d26c906503c0c/package.json), [LICENSE](https://github.com/InjectiveLabs/injective-helix-demo/blob/d916c8b20d7a269ced9fe6c7100d26c906503c0c/LICENSE)|package 1.10.3, Apache-2.0, Nuxt 3.18.1, Injective 1.16.28. 1.10.3은 package 버전이며 해당 태그 확인을 뜻하지 않는다|재사용 시 정확한 commit·lock·LICENSE/NOTICE 보존 필요|
|[nuxt.config](https://github.com/InjectiveLabs/injective-helix-demo/blob/d916c8b20d7a269ced9fe6c7100d26c906503c0c/nuxt.config.ts)|SSR=false, UI layer 기본 bdab2818f0b26f7371f14960ead6a4d39ce9399c. 환경변수 override 가능|빌드 manifest가 실제 override와 layer SHA를 고정해야 함|
|[UI layer package](https://github.com/InjectiveLabs/injective-ui/blob/bdab2818f0b26f7371f14960ead6a4d39ce9399c/package.json), [LICENSE](https://github.com/InjectiveLabs/injective-ui/blob/bdab2818f0b26f7371f14960ead6a4d39ce9399c/LICENSE)|package Apache-2.0, LICENSE MIT 불일치. Injective 1.16.24. postinstall이 Injective 패키지 latest upgrade를 요청|불일치 해소 및 설치 script 차단·lock 검증 전 제품 재사용 보류|

Helix postinstall은 외부 시장/토큰/버전 데이터를 생성하며 Bugsnag·Mixpanel·gtag·Hotjar 의존성이 있다. 해당 script를 실행하거나 Helix를 설치·빌드하지 않았다. 단순 URL 교체로 자체 chain/ML-DSA 지갑이 되지 않는다.

교체 범위는 Injective 주소/계정/wallet-* 및 TX 직렬화, 주문·취소 서명, REST/WS DTO/재연결·revision, 시세/자산 단위, 체인 네트워크·가스·후원, 잠정 잔고 및 정정 의미다. 무제한 시장가 UI는 가격 상한 IOC로 대체하거나 숨겨야 한다. 차트/상표/외부 데이터 자산의 권리도 별도 확인 대상이다.

**CTO 제안:** 재사용 불일치가 남아 있으므로 승인된 PoC 범위에서 최소 자체 화면을 우선 선택하고 Helix 컴포넌트는 출처·라이선스·의존성 확인 이후 별도 채택한다. 이번 자체 HTML은 검증 도구이며 제품 UI 채택·기능 단계 확대를 선결하지 않는다. Helix 코드를 UI에 복사하지 않았다.

## 사용 암호 라이브러리

공통 A 벡터에 맞춰 [noble-post-quantum 0.4.1](https://github.com/paulmillr/noble-post-quantum/tree/0.4.1)을 고정했다. ML-DSA의 공개 pure API는 빈 FIPS context를 사용한다. 해당 버전 d.ts의 Signer 인자 표기가 실제 context/random 인자와 달라 deterministic KAT 호출은 시험 코드 안에서만 명시적 타입을 사용한다. 제품 서명 함수는 두 인자 공개 API를 호출한다. @noble/hashes 1.8.0, @scure/base 1.2.6 및 전체 설치 integrity는 package-lock.json에 고정했다. 실험 재현용 선택이며 운영 적합성·감사 완료를 주장하지 않는다.

## 원본 설계 대조

NUS-1 개발 설계서 v1(16p) 및 아키텍처(8p)의 지갑/서명/Helix/직접 TX 항목을 읽었다. 사용자 ML-DSA 주문 권한과 체인 TX를 분리하고, 입출금·별도 송금은 사용자 직접 TX, 확정 전 수취액 재사용 금지, 서버 개인키 비보관을 유지한다. S0-A 규범이 기존 실험 JSON/U64 금액과 충돌하는 부분은 A의 protobuf 정규 바이트·U128 금액으로 대체했다. 실제 TX·API·영속 복구는 후속 승인 작업이다.
