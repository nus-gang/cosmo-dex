//! Signed-command and chain-snapshot recovery with a serialized local commit boundary.
//! No network effects. Service admission is enforced by the caller.
use super::{
    journal::{self, Commit, Error, Journal, Result, canonical, sha256},
    record::{SignedRecord, SnapshotRecord},
    sequencer::Candidate,
    snapshot::{Binding, Observation, Snapshot},
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
    status_revisions: Option<std::ops::RangeInclusive<u64>>,
}
impl SignedRecovery {
    /// Explicitly initialize a fresh namespace. A partial initialization is left
    /// intact on failure and must never be automatically recreated.
    pub fn create(path: &Path, initial: Candidate, mode: &str) -> Result<Self> {
        if initial.sequence() != 0 {
            return Err(Error::InvalidRecord("RECOVERY_BOOTSTRAP_SEQUENCE"));
        }
        initial.state_json(mode).map_err(Error::InvalidRecord)?;
        let journal = Journal::create(path, initial.snapshot().value()["body"]["context"].clone())?;
        journal.write_bootstrap(initial.snapshot().value())?;
        Self::replay(journal, Vec::new(), initial, mode)
    }
    /// Manifest-owned binding and initial snapshot ID anchor recovery. Never
    /// derive either expected value from the persisted snapshot itself.
    pub fn open_persisted(
        path: &Path,
        binding: &Binding,
        expected_snapshot_id: &str,
        mode: &str,
    ) -> Result<Self> {
        let (journal, records) = Journal::open(path, binding.context().clone())?;
        let initial = (|| {
            let raw = journal.read_bootstrap()?;
            let snapshot = binding.decode(&raw).map_err(Error::RecoveryRequired)?;
            if snapshot.id() != expected_snapshot_id || canonical(snapshot.value())? != raw {
                return Err(Error::RecoveryRequired("BOOTSTRAP_MISMATCH"));
            }
            Candidate::new(snapshot).map_err(Error::RecoveryRequired)
        })();
        match initial {
            Ok(initial) => Self::replay(journal, records, initial, mode),
            Err(error) => {
                journal.preserve_evidence()?;
                Err(error)
            }
        }
    }

    pub fn open(path: &Path, initial: Candidate, mode: &str) -> Result<Self> {
        if initial.sequence() != 0 {
            return Err(Error::InvalidRecord("RECOVERY_BOOTSTRAP_SEQUENCE"));
        }
        let context = initial.snapshot().value()["body"]["context"].clone();
        let (journal, records) = Journal::open(path, context)?;
        Self::replay(journal, records, initial, mode)
    }
    fn replay(
        journal: Journal,
        records: Vec<Value>,
        initial: Candidate,
        mode: &str,
    ) -> Result<Self> {
        let replay = (|| {
            let mut state = initial;
            let mut previous = Commit::zero();
            let mut receipts = BTreeMap::new();
            for record in records {
                let (next, signed) = match record["command_kind"].as_str() {
                    Some("ORDER" | "CANCEL" | "WITHDRAW_PREPARE" | "WITHDRAW_ABORT") => {
                        let (next, prepared) =
                            SignedRecord::replay(&state, &record, mode, &previous)
                                .map_err(Error::RecoveryRequired)?;
                        (next, Some(prepared))
                    }
                    Some("SNAPSHOT" | "CORRECTION") => {
                        let (next, _) = SnapshotRecord::replay(&state, &record, mode, &previous)
                            .map_err(Error::RecoveryRequired)?;
                        (next, None)
                    }
                    _ => return Err(Error::RecoveryRequired("REPLAY_KIND")),
                };
                let bytes = journal::frame(&canonical(&record)?)?;
                let commit = Commit {
                    command_seq: next.sequence(),
                    record_hash: sha256(&bytes),
                    end_offset: previous
                        .end_offset
                        .checked_add(bytes.len() as u64)
                        .ok_or(Error::RecoveryRequired("OFFSET_OVERFLOW"))?,
                };
                if let Some(prepared) = signed {
                    let receipt = prepared.receipt(&commit).map_err(Error::RecoveryRequired)?;
                    if receipts.insert(commit.command_seq, receipt).is_some() {
                        return Err(Error::RecoveryRequired("DUPLICATE_RECEIPT"));
                    }
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
                status_revisions: None,
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
    /// this component does not yet derive that bound.
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
        self.submit_admitted(
            kind,
            raw,
            signature,
            session_owner,
            observation,
            now,
            maximum_correction_payload_bytes,
            None,
        )
    }
    /// Service gate errors are applied after authentication and duplicate lookup,
    /// but before any new binding, journal write, or state publication.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_admitted(
        &mut self,
        kind: &str,
        raw: &[u8],
        signature: &[u8],
        session_owner: &str,
        observation: &Observation,
        now: u64,
        maximum_correction_payload_bytes: usize,
        admission_error: Option<&'static str>,
    ) -> Result<Value> {
        if self.recovery_required {
            return Err(Error::RecoveryRequired("POISONED_SESSION"));
        }
        let transition = match kind {
            "WITHDRAW_PREPARE" | "WITHDRAW_ABORT" if signature.is_empty() => self
                .state
                .local_action(kind, raw, session_owner, observation, now),
            "ORDER" | "CANCEL" => {
                self.state
                    .submit(kind, raw, signature, session_owner, observation, now)
            }
            _ => Err("UNSUPPORTED_VERSION"),
        };
        let (candidate, outcome, duplicate) = transition.map_err(Error::InvalidRecord)?;
        if duplicate {
            return self
                .receipt(session_owner, outcome.seq)
                .cloned()
                .ok_or(Error::RecoveryRequired("MISSING_ORIGINAL_RECEIPT"));
        }
        if let Some(reason) = admission_error {
            return Err(Error::InvalidRecord(reason));
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
    /// Called only by the trusted chain adapter under the same writer lock.
    /// Duplicate observations do not append. A correction uses reserved capacity;
    /// no corrected state is published until the complete journal commit succeeds.
    pub fn advance(
        &mut self,
        snapshot: Snapshot,
        observation: &Observation,
        now: u64,
        maximum_correction_payload_bytes: usize,
    ) -> Result<Option<Value>> {
        if self.recovery_required {
            return Err(Error::RecoveryRequired("POISONED_SESSION"));
        }
        let (candidate, prepared) = SnapshotRecord::prepare(
            &self.state,
            snapshot,
            observation,
            now,
            &self.mode,
            self.journal.commit(),
        )
        .map_err(Error::InvalidRecord)?;
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        if maximum_correction_payload_bytes > journal::MAX_PAYLOAD {
            return Err(Error::ResourceLimit);
        }
        journal::frame(&canonical(prepared.record())?)?;
        self.recovery_required = true;
        self.journal.append(
            prepared.record(),
            maximum_correction_payload_bytes,
            prepared.correction().is_some(),
        )?;
        self.state = candidate;
        self.recovery_required = false;
        Ok(Some(prepared.result().clone()))
    }
    /// Each published live response gets a unique revision; unused reservations
    /// are skipped on restart. Failure closes command admission as well.
    pub fn next_status_revision(&mut self) -> Result<u64> {
        if self.recovery_required {
            return Err(Error::RecoveryRequired("POISONED_SESSION"));
        }
        if self.status_revisions.as_ref().is_none_or(|range| range.is_empty()) {
            self.recovery_required = true;
            self.status_revisions = Some(self.journal.reserve_status_revisions(1024)?);
            self.recovery_required = false;
        }
        self.status_revisions.as_mut().and_then(Iterator::next).ok_or(Error::ResourceLimit)
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
