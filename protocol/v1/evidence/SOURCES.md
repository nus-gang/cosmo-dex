# 출처·재사용 범위

M0 계약 r2: a979abc3-8c56-4f12-9ba6-55fa204d8be4, SHA256 27c9170f1643cea5bca78b5bfa7b6e62a6e6ba778006ef1944669fc357e77af8. Security r1 vectors는 byte-for-byte 보존.

- [SDK key v0.55.0](https://github.com/cosmos/cosmos-sdk/blob/v0.55.0/crypto/keys/mldsa65/key.go): 2026-09-29 공식 태그 재조회, Address 및 키 길이 확인. 로컬 key.go 사본 보관.
- [SDK license](https://github.com/cosmos/cosmos-sdk/blob/v0.55.0/LICENSE), [CometBFT license](https://github.com/cometbft/cometbft/blob/v0.40.0/LICENSE), [CIRCL license](https://github.com/cloudflare/circl/blob/v1.6.3/LICENSE): M0 원문 보존; 새 runtime dependency 설치 없음.
- [Protobuf encoding](https://protobuf.dev/programming-guides/encoding/): 2026-09-29 확인. strict ordering/presence/unknown rejection은 프로젝트 추가 제약.
- noble metadata의 version/dist integrity와 LICENSE는 M0 fixture 생성 근거. 구현 담당은 실제 선택 dependency lock과 license를 다시 기록한다.

이 evidence는 법률 의견·전체 dependency 승인·제품 실행 증명이 아니다.
