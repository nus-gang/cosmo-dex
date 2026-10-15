# F15 VOID correction Apply WAL boundary

This component-only boundary uses the reviewed C/L-D Apply path. It does not
start a service or claim DEV09 completion.

- Point: `after_wal_sync`, after the complete `CORRECTION` journal frame is
  written and synced, before the commit marker changes.
- Binding: the crash reservation hashes the complete pre-Apply state, receipt
  ledger, pending batches, dependencies, and resolution receipts. The test also
  decodes the appended WAL frame and requires `command_kind=CORRECTION`, a
  non-empty correction result/closure, the original VOID receipt set, and the
  same Attempt references.
- Effect: the fault-only child calls `_exit(86)` and is reaped by its parent.
- Expected recovery: old WAL is an exact prefix of the new WAL, the marker is
  unchanged, `transaction.dev` remains, and two opens fail closed without
  changing WAL, marker, transaction, or report bytes.
- Exclusions: managed child/approval CLI, actual service/RPC/transport, three
  repetitions, physical power loss, durable ACK, and DEV PASS.

Targeted test: `tests::worker_apply_crash_replays_without_extra_batch_or_asset_effect`
with fault injection enabled. The test's fee0/fee25 × COMMITTED/VOID matrix
contains the F15 VOID cases; internal cases are not counted as separate tests.
