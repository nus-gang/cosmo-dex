# Fresh runtime initializer · NUS-73

`initialize_cli.py initialize-reviewed` is the supported L-T entry point.
It authenticates the native decision and independent CEO/CTO revisions, captures
manifest/files, profile, public keys and both descriptor-pinned executables into
private files, and supervises the bounded offline initializer/C children.
`nus-s3-local-initialize` is the dormant build-tagged child; neither binds a port.

The `create` command accepts:

- a two-field `source-input` containing the exact reviewed runtime manifest and
  its sealed files;
- the matching effective profile and independent runtime pin;
- exactly two canonical ML-DSA-65 **public** keys exported by fresh browser-held
  user keys;
- a fresh run UUID, canonical UTC genesis time, fee `0` or `25`, a private
  scratch directory, and the reviewed offline C validator;
- both opt-ins: `--local-demo-profile s3-dev-local/1` and
  `--acknowledge-unproven-space`.

It creates operator/admin and four validator/P2P identities with OS entropy in
memory, encodes app state through B's approved genesis type, builds the four
validator genesis, and calls `PrepareGuard`/`ValidateLocalDemo`. The exact
B-produced input bundle is then passed once to C's offline `validate` command
with a 60-second child deadline and bounded output. After exact C success, the
required `--publication-gate stdin` emits `INITIALIZER_READY`. The parent rereads
approvals and checks staged bytes before sending exact `PUBLISH\n` plus EOF.
Revocation, mutation, stop, timeout or EOF before that prevents publication.
IPC is sequencing, not organizational approval; the parent has a 90-second bound.

Publication creates one new root below an existing uid-owned mode-0700 parent.
Authority seeds and each validator home are created with no replacement. The
guard file is fsynced before key/config/data files; every file is mode 0600,
directories are mode 0700, and parent directories are fsynced. Failure leaves
partial evidence and the root cannot be retried. The final public
`initialization.json` contains only hashes, node IDs and paths; user private keys
never enter the process and generated private keys never enter stdout, logs,
registration packets, source bundles, or the runtime aggregate.
The root also preserves exact validated `input.json` and `effective-profile.json`
for the later Chain startup and authoritative-snapshot C bootstrap.

`registration_set_cli.py` compiles a bounded exact spec into 12 inert Paperclip
workspace packets: worker, web and four validators for each of fee0 and fee25.
It cross-checks common runtime/profile/capture pins, worker-to-validator-0 RPC,
web-to-worker bind, four peer IDs, eight loopback endpoints, private mutable
roots, resource limits, and both opt-ins. The output is canonical and includes a
detached packet-list SHA256. It does not send API requests or start services.

The actual fresh output and packet bytes are execution results and are excluded
from the contract aggregate. They can be created only after the new candidate's
CTO→Security decision and current independent CEO/CTO approvals. The first
authoritative Chain snapshot, C store bootstrap, Paperclip registration and all
service START operations remain NUS-74.

## Approved B integration

NUS-73 consumes the reviewed NUS-55 candidate
`fb4addfe1bcf9a8c39837b8a8dc199eed9481f27` without changing its five approved
files. `ValidateLocalDemo` now requires the public account receipt
manifest/schema/version fields and the inherited public file set. The SRE
fixture uses A head `ed4cf278cff78312ac606d6834901e0b8b265725`; the resulting
contract hash therefore covers both the original local-demo inputs and the
public receipt overlay.

The initializer still derives the Context only through B's
`PrepareGuard`/`ValidateLocalDemo` path and passes the exact B-produced bundle to
C. There is no old-manifest fallback, guard reissue, or migration path. Actual
runtime approval remains dependent on the newly sealed five descriptors,
contract aggregate, build artifacts, native CTO→Security decision, and current
independent CEO/CTO approval documents.

## Component verification

These are pre-service component tests only:

```sh
cd chain/app
GOTOOLCHAIN=local go test -mod=readonly -tags dev_local_demo \
  ./internal/localkeys ./cmd/nus-s3-local-initialize

cd ../../ops/s3-local
python3 -B -m unittest test_registration_set -v
```

`service_started=false`, `approval_verified=false`, `durable_ack=false`, and
DEV01–DEV14 remain `NOT_RUN`.

## 실행 순서와 키 준비

정상 웹은 기존 C home을 요구하므로 genesis 전에 명시적 키 준비 단계를 사용한다.
`key_preparation_cli.py prepare-keys-reviewed`는 같은 승인 웹 자산의 `/`와
`/page.js`만 제공한다. Context/인증/REST/Chain RPC/proxy는 제공하지 않는다.
기존 `LocalKey`·탭 수명·두 opt-in을 그대로 사용한다.

1. 새 native CTO→Security 및 각 CEO/CTO canonical 승인 뒤, 현재 run 인증으로
   `registration_render_cli.py key-packets-reviewed --input SPEC`를 실행한다.
   두 packet은 최종 웹 workspace와 동일 이름/command ID다. 권한 있는 경로에서
   fee0 웹 workspace만 등록하고 Paperclip 관리 runtime으로 시작한다. current-run
   broker·PID mailbox는 기존 PRIVATE-READER/PID 수명 규칙을 따른다.
2. 새 탭에서 두 opt-in과 ‘새 시험 계정 2개 준비’를 선택한다. 공개 등록 JSON의
   `public_key_base64` 두 필드만 JSON 문자열 배열 파일로 추출한다. 비밀키는
   탭 밖으로 나오지 않는다. 탭을 새로고침/닫지 않는다.
3. 관리 runtime의 준비 서버를 종료하고 PID/5173 해제를 확인한다. 탭은 유지한다.
   `initialize_cli.py initialize-reviewed`에 bundle/artifacts/runtime-pin,
   native-decision-id/ceo-revision/cto-revision, effective-profile, user-public-keys,
   output, scratch, run-uuid, genesis-time, fee-bps 및 두 opt-in을 전달한다.
   output 부모는 canonical uid-owned0700이며 output은 없어야 한다.
4. `registration_render_cli.py packets-reviewed --input SPEC`는 실제 두 발행
   보고서와 입력 pin/genesis/guard를 대조하여 최종12 packet을 렌더링한다. fee별
   worker는 `authority/operator-0`을 사용한다. control_root는 Unix socket
   제한103 bytes 안의 짧은 전용 private 경로다. RPC/P2P는 fee0=26656..26663,
   fee25=26756..26763, worker는18080/18081, 웹은5173이다. 가용성은 시작 직전
   기존 port preflight로 검사하며 렌더러는 포트를 예약하지 않는다.
5. 승인 Chain 네 개를 관리 runtime으로 시작한 뒤 실제 authoritative snapshot으로
   기존 bootstrap-fetch/create 경로의 C `engine` home을 만든다. genesis를
   snapshot으로 위장하지 않는다. worker를 시작한다. 정지한 동일 웹 workspace의
   설정을 최종 web packet으로 권한 있는 경로에서 갱신·대조한 후 정상 웹을 시작한다.
   유지한 탭의 ‘초기화 후 화면 연결’로 새 Context를 받는다.
6. fee0 종료·PID/port/writer 관측과 증거 보존 후 fee25로 반복한다. 두 fee는
   승인 origin5173을 공유하므로 동시에 실행하지 않는다. 각 fee의 별도 탭을
   유지하고 새 사용자·authority·검증인을 사용한다. 부분 게시에서는 home을
   보존하며 자동 수리/재발급하지 않는다. 실제 실행은 NUS-74다.

렌더러 입력 schema는 `s3-local-initializer-registration/1`이다. 필드는 `python`,
`candidate`, `bundle`, `artifacts`, `runtime_pin`, `native_decision_id`, `ceo_revision`,
`cto_revision`, `control_root`, `profiles`이며 profiles는 실제 발행 root를
가리키는 fee0/fee25 map이다. 두 키 준비 탭·initializer를 먼저 완료하여 둘의
공개 보고서를 준비할 수 있다. 두 준비 서버도 차례로 실행하고 탭은 유지한다.
최종 서비스는 fee0 종료 후 fee25 순서다.

승인 전에는 새 decision/revision이 존재하지 않으므로 서비스용 packet을 임의
값으로 발급하지 않는다. 검토물의 합성 packet은 렌더링 검증 증거로만 사용한다.
