# 초기 Snapshot 조회 child (L-T 전용 내부 진입점)

`runtime/bootstrap_fetch.rs`는 기존 `bootstrap::fetch`/`QueryRpc`를 호출한다.
캡처 SHA·실효 profile·runtime pin·두 opt-in을 C `Validated`로 검사한 뒤,
그 Context로 Snapshot(height=0) 요청을 한 번 구성한다. DNS/proxy/redirect 및
재시도 없이 명시적 loopback SocketAddr를 사용한다. 기존 RPC transport의 총
2초 및 16MiB 응답 상한을 유지한다. 이 명령은 home/key/서비스를 만들지 않는다.

```text
s3-local-bootstrap-fetch fetch-captured --chain-rpc 127.0.0.1:26657 \
  --capture-sha256 CAPTURE_SHA256 --runtime-pin RUNTIME_PIN \
  --local-demo-profile /absolute/effective-profile.json \
  --acknowledge-unproven-space
```

stdin은 bounded capture 원문, stdout은 성공한 RPC 응답 원문만이다. 원문을
파싱/재직렬화하거나 JSON 성공 보고서로 감싸지 않는다. 오류는 exit2와 고정
`LOCAL_BOOTSTRAP_FETCH_REJECTED` 진단이다. stdout write/flush 실패는 일부
원문을 남길 수 있으므로 부모는 종료코드와 수신 완료를 확인해야 한다.
조회 중 stop이 도착했더라도 이미 받은 원문은 보존을 위해 전달한다. 이를
create 허가로 해석하지 않으며 부모의 후속 stop/승인 검사가 반드시 필요하다.

아직 외부 직접 실행용 launcher가 아니다. 부모는 실행 직전 인증 승인 조회,
동일 descriptor/SHA의 private 실행 사본, 유한 stdin/출력/수명 감독 및
kill/reap, 원문 증거 fsync 게시, create 직전 재승인을 연결해야 한다.
이 부모 연결과 최종 descriptor 반영은 남아 있다. 조직 승인/최종 pin 미발급.

이번 L-R 검증은 컴파일, fee0/25 C 입력 검증과 주입 callback, 잘못된 CLI의
실제 subprocess 거절뿐이다. 실제 RPC/서비스 호출0. 정상 네트워크 조회와
4검증인 통합은 승인 runtime pin 이후 L-T에서 수행한다.
