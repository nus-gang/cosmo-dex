//! Signed-prefix recovery. No network effects or admission are performed here.
//! The caller supplies the verified bootstrap snapshot, not the last embedded
//! state. Internal snapshot/withdraw/correction records are deliberately rejected
//! until their semantic replay is implemented. This is not yet service startup.
use super::{
    journal::{self, Commit, Error, Journal, Result, canonical, sha256},
    record::SignedRecord,
    sequencer::Candidate,
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub struct SignedRecovery {
    // Retain the OS lock throughout the recovered state's lifetime.
    journal: Journal,
    state: Candidate,
    receipts: BTreeMap<u64, Value>,
}
impl SignedRecovery {
    pub fn open(path: &Path, initial: Candidate, mode: &str) -> Result<Self> {
        if initial.sequence() != 0 {
            return Err(Error::InvalidRecord("RECOVERY_BOOTSTRAP_SEQUENCE"));
        }
        let context = initial.snapshot().value()["body"]["context"].clone();
        let (journal, records) = Journal::open(path, context)?;
        let replay = (|| {
            let mut state = initial;
            let mut previous = Commit::zero();
            let mut receipts = BTreeMap::new();
            for record in records {
                let (next, prepared) = SignedRecord::replay(&state, &record, mode, &previous)
                    .map_err(Error::RecoveryRequired)?;
                let bytes = journal::frame(&canonical(&record)?)?;
                let commit = Commit {
                    command_seq: next.sequence(),
                    record_hash: sha256(&bytes),
                    end_offset: previous
                        .end_offset
                        .checked_add(bytes.len() as u64)
                        .ok_or(Error::RecoveryRequired("OFFSET_OVERFLOW"))?,
                };
                let receipt = prepared.receipt(&commit).map_err(Error::RecoveryRequired)?;
                if receipts.insert(commit.command_seq, receipt).is_some() {
                    return Err(Error::RecoveryRequired("DUPLICATE_RECEIPT"));
                }
                previous = commit;
                state = next;
            }
            if &previous != journal.commit() {
                return Err(Error::RecoveryRequired("RECOVERY_FINAL_COMMIT"));
            }
            Ok((state, receipts))
        })();
        match replay {
            Ok((state, receipts)) => Ok(Self {
                journal,
                state,
                receipts,
            }),
            Err(error) => {
                journal.preserve_evidence()?;
                Err(error)
            }
        }
    }
    pub fn state(&self) -> &Candidate {
        &self.state
    }
    pub fn commit(&self) -> &Commit {
        self.journal.commit()
    }
    /// `owner` must come from the authenticated session, never an unchecked URL.
    /// The original receipt is immutable even if the current order was cancelled.
    pub fn receipt(&self, owner: &str, sequence: u64) -> Option<&Value> {
        self.receipts.get(&sequence).filter(|r| r["owner"] == owner)
    }
}
