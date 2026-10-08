# Fresh runtime initializer · NUS-73

`nus-s3-local-initialize` is the dormant, build-tagged L-R entry point for
creating one new fee profile. It starts no process and binds no port.

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
with a 60-second parent deadline and bounded output. Only the exact C success
line opens publication.

Publication creates one new root below an existing uid-owned mode-0700 parent.
Authority seeds and each validator home are created with no replacement. The
guard file is fsynced before key/config/data files; every file is mode 0600,
directories are mode 0700, and parent directories are fsynced. Failure leaves
partial evidence and the root cannot be retried. The final public
`initialization.json` contains only hashes, node IDs and paths; user private keys
never enter the process and generated private keys never enter stdout, logs,
registration packets, source bundles, or the runtime aggregate.

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

## Current blocker

The reviewed B `ValidateLocalDemo` still accepts the old six-field runtime
manifest. The current runtime preflight requires the public account receipt
manifest/schema/version fields and inherited file set. An exact new manifest is
therefore rejected before any home is created with:

```text
json: unknown field "public_receipt_manifest_sha256"
```

This is a B contract-integration change, not launcher policy. NUS-55 must add the
three pinned fields and public receipt file-set aggregation, prove that the new
contract hash reaches dynamic Context/genesis/restart validation, and complete
a new CTO→Security review. L-R must then consume that approved B candidate and
rerun the initializer and final bundle tests. No SRE fallback is permitted.

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
