# F16 VOID correction publish boundary

This component-only boundary uses the reviewed C/L-D Apply path. It does not
start a service or claim DEV09 completion.

- Point: `before_publish`, after WAL and marker durability and removal of the
  object transaction, before the child replaces the complete Arc reader.
- Effect: the fault-only child calls `_exit(86)` and is reaped by its parent.
- Expected recovery: two opens reproduce the exact state encoded in the durable
  correction record. Full-state equality covers accounts/holds, order/FIFO,
  fills, dependencies, correction closure, cursor, and original VOID receipt.
  The development receipt ledger preserves every prior entry and adds exactly
  one Apply receipt with `durable_ack=false`.
- Exclusions: a concurrent external reader barrier, HTTP/UI response loss,
  managed child/approval CLI, three repetitions, physical power loss, and DEV
  PASS.

Targeted test: `tests::worker_apply_crash_replays_without_extra_batch_or_asset_effect`
with fault injection enabled. The test's fee0/fee25 × COMMITTED/VOID matrix
contains the F16 VOID cases; internal cases are not counted as separate tests.
