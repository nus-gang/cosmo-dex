# L-R Chain 진입점 중간 인계

`chain/app/cmd/nus-s3-local-chain`은 별도 `dev_local_demo` build tag로만 빌드한다. B `ValidateLocalDemo`/`NewLocalDemo`를 호출하며 원 B·경제 로직·lock 파일을 변경하지 않는다. **최종 runtime 후보가 아니며 지금 서비스를 시작하지 않는다.** 전체 launcher와 fee0/25 초기화, 독립 runtime pin, CTO→Security 심사는 남아 있다.

## 빌드·검증

기존 Go1.26.5 toolchain/cache에서 다음을 실행한다. `GOCACHE`와 `GOTMPDIR`는 담당자의 격리 경로다.

```sh
cd chain/app
GOTOOLCHAIN=local GOPROXY=off GOSUMDB=off go build -mod=readonly \
  -tags dev_local_demo -o /existing-build-root/nus-s3-local-chain ./cmd/nus-s3-local-chain
GOTOOLCHAIN=local GOPROXY=off GOSUMDB=off go test -mod=readonly \
  -tags dev_local_demo ./cmd/nus-s3-local-chain -count=1 -v
```

시험은 9개다. 두 opt-in·중복/알 수 없는 옵션, 중복 JSON, 파일 크기/권한/심볼릭·하드링크, literal loopback, 공개 RPC/P2P 옵션 비활성, writer2, lock 해제, 거절 시 home 생성0, 합성 home의 키/guard 바인딩과 preflight 무부작용을 확인한다. 시험의 임시 검증인 키·genesis는 실제 runtime 입력이 아니며 시험 종료에 정리한다. socket·DB·Comet node는 시작하지 않는다. 실제 4검증인 종료·port 해제·restart는 L-T NOT_RUN이다.

## 실행 인자 계약

명령은 `preflight`와 `start`뿐이다. 두 명령 모두 다음을 명시해야 한다.

- `--local-demo-profile <absolute canonical effective profile path>`와 `--acknowledge-unproven-space`
- `--input-set <absolute canonical bundle path>`: C `Validated::decode_bundle`과 같은 4필드 JSON(`runtime_manifest`, `files`, `guard`, `genesis`). byte는 base64다.
- `--runtime-pin <independently approved SHA256>`: 입력에서 스스로 pin을 발급하지 않는다. 이 CLI는 바이트만 검증하며 승인 판정을 대신하지 않는다.
- `--home <absolute canonical validator home>`: root/config/data 0700·현재 UID. genesis/guard는 bundle과 정확히 같아야 한다. 키·서명 상태는 0600 regular single-link 파일이어야 한다.
- `--rpc 127.0.0.1:<port>`와 `--p2p 127.0.0.1:<port>`: literal loopback와 1024..65535만 허용한다. hostname·scheme·공개 주소·port0을 거절한다.
- `--peers <node-id@loopback:port,node-id@loopback:port,node-id@loopback:port>`: 서로 다른 3개 peer/주소. 자기 node ID를 거절한다.

`preflight`는 이미 준비된 home을 읽기만 한다. 승인 B 입력 검증, 로컬 home의 genesis/guard 일치, 검증인 private/public/address·genesis validator 포함 관계, Comet 서명 상태와 node key를 검증한다. runtime 승인이나 실제 포트 가용성을 주장하지 않는다. launcher가 새 root 생성·guard fsync/no-replace·포트·자원 상한을 담당해야 한다.

`start` 배선은 동일 검증 후 `writer.dev.lock`을 nonblocking flock으로 잡고 다시 바인딩을 읽은 뒤, 검토된 B 생성자로 application DB를 연다. Comet node를 시작하고 SIGINT/SIGTERM에 Stop/Wait 후 DB·lock을 닫는다. lock inode는 지우지 않는다(삭제 후 재생성으로 단일 writer가 깨지는 것을 방지). C engine과는 별도 validator home이다. 기존 config.toml에서 공개 listener·peer를 가져오지 않는다. RPC unsafe/CORS/GRPC/pprof·Prometheus·P2P discovery는 비활성이다. 기존 노드의 키·서명 상태를 자동 생성·초기화하지 않는다.

`start`는 구현된 코드 경로일 뿐 이번 L-R에서 실행하지 않았다. L-T의 실제 기동은 Paperclip 관리 runtime 등록과 승인 manifest 대조 후에만 한다. `durable_ack=false`, 표준 `G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`를 유지한다.

## 남은 통합 작업

1. fee0/25 네 validator·새 합성 계정 genesis/home 초기화, guard no-replace/fsync, runtime 자원/포트와 관리 프로세스 목록.
2. C/Worker/Rest의 신뢰 관측·확정 proof bridge와 bounded HTTP listener, 웹 ChainPort·mount.
3. launcher/preflight/fault/증거 보존·정리 명령, 최종 binary·웹 목록 및 5 descriptor manifest.
4. exact 후보 독립 CEO/CTO 승인 출처와 CTO→Security 심사, L-T 인계.
