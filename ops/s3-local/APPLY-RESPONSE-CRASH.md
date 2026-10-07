# F12 Apply response boundary

This component-only boundary exercises the reviewed C/L-D Apply path. It does
not start a service or claim DEV09 completion.

- Point: `before_response`, after the Apply WAL and marker are durable and the
  complete state plus development receipt ledger are published, before the
  caller receives the receipt.
- Effect: the fault-only child calls `_exit(86)`; the parent reaps it. A
  reserved-only crash report remains `UNKNOWN` and is not proof that the caller
  observed a response.
- Expected recovery: the new WAL and marker remain unchanged, no
  `transaction.dev` remains, and two opens of the same home return the same
  commit, full state, and receipt ledger. The old receipt entries are exact
  prefixes of the ledger and exactly one Apply entry is added with
  `durable_ack=false` and `LOCAL_WRITE_COMPLETED_UNPROVEN_SPACE`.
- Profiles: isolated synthetic fee0 and fee25 fixtures, for both COMMITTED and
  VOID Apply; two opt-ins are required.
- Exclusions: actual service/RPC/HTTP response loss, browser receipt comparison,
  three repetitions, physical power loss, and DEV PASS.

Targeted test:

```sh
rustc --edition=2024 --test --cfg 'feature="fault-injection"' \
  ops/s3-local/runtime/submit.rs -L dependency=<offline-target>/debug/deps \
  --extern nus_exchange_contract=<offline-target>/debug/deps/libnus_exchange_contract-<hash>.rlib \
  '<the remaining locked dependency --extern arguments>' -o <scratch>/f12-submit-tests
<scratch>/f12-submit-tests --exact \
  tests::worker_apply_crash_replays_without_extra_batch_or_asset_effect \
  --test-threads=1 --nocapture
```

The evidence bundle records the expanded `rustc` command and locked rlib
hashes. No dependency is downloaded and no manifest or lockfile is changed.
