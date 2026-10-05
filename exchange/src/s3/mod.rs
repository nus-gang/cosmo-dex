//! S3 private engine components. These are not a receipt verifier or a public
//! service. Accounting/graph transitions must be authorized and committed by the
//! integrated sequencer before publication. S2 journals/outbox are never imported.
pub mod dependencies;
pub mod journal;
pub mod ledger;
