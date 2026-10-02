//! Signed-prefix recovery and serialized local commit boundary. No network effects.
//! The caller supplies the verified bootstrap snapshot, not the last embedded
//! state. Internal snapshot/withdraw/correction records are deliberately rejected
//! until their semantic replay is implemented. This is not yet service startup.
use super::{
    journal::{self, Commit, Error, Journal, Result, canonical, sha256},
    record::SignedRecord,
    sequencer::Candidate,
    snapshot::Observation,
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub struct SignedRecovery {
    // Retain the OS lock throughout the recovered state's lifetime.
    journal: Journal,
    state: Candidate,
    receipts: BTreeMap<u64, Value>,
    mode: String,
    recovery_required: bool,
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
                mode: mode.into(),
                recovery_required: false,
            }),
            Err(error) => {
                journal.preserve_evidence()?;
                Err(error)
            }
        }
    }
    /// The transport must authenticate `session_owner` and enforce the global
    /// admission gate before calling this serial (&mut self) commit boundary.
    /// The capacity argument must be a proven conservative correction bound;
    /// this component does not yet derive that bound or permit internal events.
    /// No candidate state or receipt escapes before the journal fsync completes.
    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &mut self,
        kind: &str,
        raw: &[u8],
        signature: &[u8],
        session_owner: &str,
        observation: &Observation,
        now: u64,
        maximum_correction_payload_bytes: usize,
    ) -> Result<Value> {
        if self.recovery_required {
            return Err(Error::RecoveryRequired("POISONED_SESSION"));
        }
        let (candidate, outcome, duplicate) = self
            .state
            .submit(kind, raw, signature, session_owner, observation, now)
            .map_err(Error::InvalidRecord)?;
        if duplicate {
            return self
                .receipt(session_owner, outcome.seq)
                .cloned()
                .ok_or(Error::RecoveryRequired("MISSING_ORIGINAL_RECEIPT"));
        }
        let prepared = SignedRecord::prepare(
            &self.state,
            &candidate,
            &outcome,
            kind,
            observation,
            now,
            &self.mode,
            self.journal.commit(),
        )
        .map_err(Error::InvalidRecord)?;
        // Preflight has no disk effects. A capacity refusal must not bind the ID.
        if maximum_correction_payload_bytes > journal::MAX_PAYLOAD {
            return Err(Error::ResourceLimit);
        }
        journal::frame(&canonical(prepared.record())?)?;
        // Once append begins any failure is UNKNOWN and requires reopening.
        // In particular a failed marker fsync must never become REJECTED.
        self.recovery_required = true;
        let commit =
            self.journal
                .append(prepared.record(), maximum_correction_payload_bytes, false)?;
        let receipt = prepared.receipt(&commit).map_err(Error::RecoveryRequired)?;
        if self.receipts.contains_key(&commit.command_seq) {
            return Err(Error::RecoveryRequired("DUPLICATE_RECEIPT"));
        }
        self.receipts.insert(commit.command_seq, receipt.clone());
        self.state = candidate;
        self.recovery_required = false;
        Ok(receipt)
    }
    pub fn recovery_required(&self) -> bool {
        self.recovery_required
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
