# S1 계약 심사 패키지

규범은 CONTRACT.md, 신규 SDK 메시지 tag는 messages.proto. S0는 변경하지 않는다.

검증(저장소 루트):

```sh
python3 protocol/s1/tools/check.py
cd chain
GOTOOLCHAIN=local go run -mod=readonly ../protocol/s1/tools/sign-vectors.go
```

Go 명령은 S0 Go 1.24.4/CIRCL v1.6.3으로 암호 벡터만 확인한다. S1 앱 Go 1.26.5/SDK 빌드 시험이 아니다. 쓰기 가능한 GOMODCACHE/GOCACHE를 지정할 수 있다.

`generate.py`와 `sign-vectors.go --generate`는 명시적인 벡터 재생성용이며 검증 명령이 아니다. golden을 변경하면 전체 파일 hash와 독립 검토를 갱신한다. 모든 seed/genesis fixture는 공개 합성 값이다.

재생성 순서는 generate.py → chain에서 sign-vectors.go --generate → 루트에서 tools/seal.py이다. seal.py는 TxRaw/hash와 manifest를 기록한다. 정상 check는 파일을 수정하지 않는다. verification.txt는 실행 로그라 manifest 자기참조 대상에서 제외한다.
