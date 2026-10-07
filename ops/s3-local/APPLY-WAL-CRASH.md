# F11 Apply WAL sync crash boundary

This component-only boundary exercises the existing reviewed C/L-D Apply path.
It does not start a service or claim DEV09 completion.

- Point: `after_wal_sync`, after the new Apply record is fully written and
  `fsync` has returned, before the commit marker is replaced.
- Effect: the fault-only child calls `_exit(86)`; the parent reaps it.
- Evidence: `storage-crash.jsonl` contains the canonical Apply command, commit,
  Context, Snapshot, observation, point, occurrence, and SHA-256. A reserved-only
  report is `UNKNOWN`, never proof that the crash or Apply succeeded.
- Expected recovery: the old WAL bytes remain an exact prefix of the longer WAL,
  the old marker is unchanged, and `transaction.dev` remains. Two opens of the
  same home must fail closed with `UNKNOWN_OR_INCOMPLETE_STORE` without changing
  WAL, marker, transaction, or report bytes.
- Profiles: isolated synthetic fee0 and fee25 fixtures; two opt-ins are required.
- Exclusions: actual service/RPC/transport, concurrent reader timing, UI receipt
  ledger comparison, three repetitions, physical power loss, and DEV PASS.

Targeted test:

```sh
rustc --edition=2024 --test --cfg 'feature="fault-injection"' \
  ops/s3-local/runtime/submit.rs -L dependency=<offline-target>/debug/deps \
  --extern nus_exchange_contract=<offline-target>/debug/deps/libnus_exchange_contract-<hash>.rlib \
  '<the remaining locked dependency --extern arguments>' -o <scratch>/f11-submit-tests
<scratch>/f11-submit-tests --exact \
  tests::worker_apply_crash_replays_without_extra_batch_or_asset_effect \
  --test-threads=1 --nocapture
```

The evidence bundle records the expanded exact `rustc` command and selected
locked rlib hashes. No dependency was downloaded and no manifest or lockfile
was changed.
