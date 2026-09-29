# rc4 G-FIX-01 TS 인계

공통 기준선 `57c187f5474e02c3f624667d3b8380268e13dd1a`의 protocol 파일을 그대로 소비한다. NUS-10 Security→QA completed/approved 확인 후 작업했다.

- contract: `afa3471a2210a90cde0004b3219b231468bf0b8471e8068f8e16b096b3549d84`
- vectors: `bb1b437d23a365f1083e85353bf62bcef6517cda94288ff7e2d0b4cb5fccd55b`
- 기존 392 assertions + 새 공통 60개 전체 출력 사례 + 별도 null epoch/false flag 1개. Node·실제 Chrome 각각 453 PASS, build/typecheck PASS.
- 새 60개는 authenticated_order projection을 실제 OrderV1에 적용해 재직렬화·ML-DSA 재서명 후 decideOrder로 처리한다. 기대 authentication을 주입하지 않는다. 인증 거절은 signature bit 변조, 미연결은 등록 키 type 누락으로 유발한다. 개인키는 메모리에만 있고 finally에서 지운다.
- 원본 snapshot ID는 부분 누락·모순·인증 거절에도 유지한다. context epoch=null과 flag=false 조합의 undefined-only guard를 string 검사로 보강했다. 추가 회귀는 공통 60개와 별도 집계한다.
- 기존 수수료·cap·정수·wire·키·epoch·enum 회귀 유지. 기존 의존성을 사용했으며 fresh 설치 증거가 아니다.

## 재현

```sh
cd web
npm test
npm run build
EVIDENCE_DIR=evidence/rc4 npm run test:browser
cd ..
python3 protocol/v1/tools/check.py
```

CHROME_BIN으로 브라우저 경로를 지정한다. 기본 macOS Google Chrome, CI Linux Chrome. standalone HTML에서 key generation·서명·메모리 복구·390px overflow 및 외부 요청 0을 확인한다.

## 판정 경계

60개 TS 실제 인증 경로 PASS는 60×3 언어 비교가 아니다. 새 Go/Rust 교차 재시험 NOT_RUN. 기존 NUS-16의 783/782/1 공통 출력 FAIL·시험한 인증/거절 및 암호 PASS와 이전 759/754/5 이력을 보존한다. CTO가 수정 SHA/hash를 고정한 뒤 기존 G 전체 재시험과 H fresh checkout/공통 CI 인수로 연결한다.

ACK·원장·REST/WS·체인 NOT_CONNECTED, WAL NOT_RUN, 영속 백업 미구현. main 미병합. 후속 기능·출시·실자산 승인 없음. 새 CTO→Security 네이티브 검토를 요청하고 과거 승인 decision 이력을 보존한다.
