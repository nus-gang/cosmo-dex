# 새 Comet 키 생성 경계

`chain/app/internal/localkeys`는 `dev_local_demo` build에서만 제공한다.
`GenerateFour()`는 OS entropy로 검증인 서명 키 4개와 별도 P2P 키 4개를
메모리에서 생성한다. fee0/fee25는 반드시 별도 호출하며 키를 공유하지 않는다.
공개 fixture seed 또는 키를 받는 production API는 없다.

`Material.Public()`는 genesis용 공개 검증인 정보(power=10)와 peer ID의 사본이다.
`PrivateFiles()`는 `priv_validator_key.json`, `node_key.json`, 초기 높이 0의
`priv_validator_state.json` 바이트 사본이다. 이 반환값은 private home 게시기에만
전달하고 응답·로그·artifact에 넣지 않는다. Material 자체의 JSON 직렬화는
거절하며 기본 fmt 출력은 REDACTED다. 명시적으로 꺼낸 private bytes의 보호는
호출자 책임이고 메모리 zeroization 보장은 없다.

아직 genesis 조립·공개 사용자 등록 자료·ML-DSA operator 키 생성·B/C 의미 검증과
연결되지 않은 내부 API다. 파일/DB/listener를 생성하지 않으며 runtime 승인을
부여하지 않는다. 검증된 genesis와 guard를 결합한 뒤에만 기존
`fresh_chain_home.publish`에 전달해야 한다. 사용자/운영자 ML-DSA 키와 Comet
Ed25519 키를 혼용하지 않는다.

검증: 설치 Go 1.26.5, CometBFT v0.40.0, 변경 없는 go.mod/go.sum,
`GOTOOLCHAIN=local GOPROXY=off GOSUMDB=off go test -mod=readonly -tags dev_local_demo -count=1 -v ./internal/localkeys`
(chain/app에서 실행). 신규3 PASS: 두 생성 집합의16키 중복 없음·공개/비공개
Comet decoder 결합·서명 roundtrip, entropy 오류/중복의 결과 폐기,
반환값 alias 차단·기본 직렬화/로그 비노출. 실제 서비스/체인 시험은 아니다.
