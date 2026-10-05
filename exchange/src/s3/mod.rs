//! Experimental S3 components and private deterministic candidates. No public
//! service, durable command ACK, or integrated semantic WAL replay is provided.
//! Publication/capacity awaits the approved raw-evidence storage contract (NUS-65).
//! Callers must not treat candidate transitions as durable application results.
pub mod dependencies;
pub mod journal;
pub mod ledger;

pub mod engine;
pub mod proof;
pub mod schema;
pub mod sequencer;
pub mod snapshot;
pub mod wire;
