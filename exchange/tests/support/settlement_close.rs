//! CLOSE component regression: real C store + ML-DSA, synthetic chain evidence.
use super::*;
use nus_exchange_contract::s3::{
    dev_local::{Engine as DevEngine, Error},
    evidence,
    settlement_local::{Worker, chain::OperatorSigner},
};
use std::{cell::Cell, sync::Arc};
struct Signing {
    pk: Vec<u8>,
    calls: Cell<usize>,
}
impl Signing {
    fn new() -> Self {
        Self {
            pk: hex::decode(key(16)["public_key_hex"].as_str().unwrap()).unwrap(),
            calls: Cell::new(0),
        }
    }
}
impl OperatorSigner for Signing {
    fn public_key(&self) -> &[u8] {
        &self.pk
    }
    fn sign(&self, doc: &[u8]) -> nus_exchange_contract::s3::dev_local::Result<Vec<u8>> {
        self.calls.set(self.calls.get() + 1);
        Ok(sign(16, doc))
    }
}
struct Never;
impl OperatorSigner for Never {
    fn public_key(&self) -> &[u8] {
        panic!("pre-sign gate reached operator key")
    }
    fn sign(&self, _: &[u8]) -> nus_exchange_contract::s3::dev_local::Result<Vec<u8>> {
        panic!("pre-sign gate reached signer")
    }
}
fn rejected(bps: u32) -> (Candidate, Value) {
    let (c, _) = failure_recovery::failed(bps);
    let original = c.rejection_evidence().unwrap();
    let c = replay_check(&c, c.reject_final(original.clone()).unwrap(), "VOID_BATCH");
    (c, original)
}
fn take_engine(restart: bool) -> Arc<DevEngine> {
    TRACE.with_borrow_mut(|t| {
        let t = t.as_mut().unwrap();
        let e = t.dev.engine.take().unwrap();
        if restart {
            drop(e);
            Arc::new(DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap())
        } else {
            Arc::new(e)
        }
    })
}
fn check_replays(
    before: &nus_exchange_contract::s3::dev_local::View,
    hash: &str,
    original: &Value,
    label: &str,
) {
    TRACE.with_borrow(|t| {
        let t = t.as_ref().unwrap();
        for _ in 0..2 {
            let e = DevEngine::open(&t.dev.home, t.dev.config.clone()).unwrap();
            let after = e.reader().get().unwrap();
            assert_eq!(after.commit, before.commit);
            assert_eq!(after.state, before.state);
            assert_eq!(after.receipts, before.receipts);
            let r = e
                .trusted_recovery_failure(
                    &after.commit,
                    original["batch"]["batch_id"].as_str().unwrap(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(r.raw, canonical(original).unwrap());
            let a = e
                .trusted_recovery_attempt(&after.commit, hash)
                .unwrap()
                .unwrap();
            assert_eq!(a.attempt["kind"], "CLOSE");
            assert_eq!(
                sha256(
                    a.evidence
                        .resolve(&a.attempt["raw_tx_ref"], evidence::TX)
                        .unwrap()
                ),
                hash
            );
        }
        dev_fixture::copy_home(label, &t.dev.home);
    });
}
#[test]
fn close_signed_exact_persisted_unknown_and_two_replays() {
    for bps in [0, 25] {
        let (mut c, original) = rejected(bps);
        let id = original["batch"]["batch_id"].as_str().unwrap();
        let expected = attempt(&mut c, "CLOSE", Some(&original));
        let e = take_engine(true);
        let w = Worker::new(e.clone());
        let before = e.reader().get().unwrap();
        let signer = Signing::new();
        let hash = w
            .prepare_close(
                &before.commit,
                c.latest(),
                id,
                1,
                16,
                0,
                &signer,
                &observation(&c),
                NOW,
            )
            .unwrap();
        assert_eq!(hash, expected["tx_hash"]);
        assert_eq!(signer.calls.get(), 1);
        let after = e.reader().get().unwrap();
        assert_eq!(after.commit.command_seq, before.commit.command_seq + 1);
        for k in [
            "accounts",
            "corrections",
            "resolution_receipts",
            "chain_snapshot",
        ] {
            assert_eq!(after.state[k], before.state[k]);
        }
        let r = e
            .trusted_recovery_attempt(&after.commit, &hash)
            .unwrap()
            .unwrap();
        assert_eq!(r.attempt, expected);
        let tx = r
            .evidence
            .resolve(&r.attempt["raw_tx_ref"], evidence::TX)
            .unwrap();
        assert_eq!(
            tx,
            c.evidence_set()
                .unwrap()
                .resolve(&expected["raw_tx_ref"], evidence::TX)
                .unwrap()
        );
        assert_eq!(
            nus_exchange_contract::s3::wire::attempt_envelope(
                &r.attempt,
                &r.evidence,
                c.batch_wire(id).unwrap()
            )
            .unwrap(),
            Some((
                original["failed_tx_hash"].as_str().unwrap().into(),
                schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", &original).unwrap()
            ))
        );
        // PREPARED is not permission to create a new envelope or release D/P.
        assert!(matches!(
            w.prepare_close(
                &after.commit,
                c.latest(),
                id,
                2,
                16,
                1,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Invalid("ATTEMPT_UNRESOLVED"))
        ));
        let mut callbacks = 0;
        w.test_broadcast(&hash, &observation(&c), NOW, |a, raw| {
            callbacks += 1;
            assert_eq!(a["kind"], "CLOSE");
            assert_eq!(a["state"], "SUBMISSION_UNKNOWN");
            assert_eq!(a["broadcast_count"], "1");
            assert_eq!(raw, tx);
        })
        .unwrap();
        assert_eq!(callbacks, 1);
        let final_view = e.reader().get().unwrap();
        assert!(matches!(
            w.prepare_close(
                &final_view.commit,
                c.latest(),
                id,
                2,
                16,
                1,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Invalid("ATTEMPT_UNRESOLVED"))
        ));
        drop(w);
        drop(e);
        check_replays(&final_view, &hash, &original, "close-signed");
        dev_fixture::evidence(
            &format!("close-signed-fee{bps}"),
            &json!({"result":"PASS","scope":"COMPONENT_SYNTHETIC_RPC_REAL_STORE","signer_calls":1,"callbacks":callbacks,"tx_hash":hash,"attempt":expected,"tx_raw_base64":STANDARD.encode(tx),"failure_evidence_hash":schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", &original).unwrap(),"replays":2}),
        );
    }
}
#[test]
fn close_presign_rejects_state_stale_budget_and_wrong_batch() {
    for bps in [0, 25] {
        let (c, original) = rejected(bps);
        let id = original["batch"]["batch_id"].as_str().unwrap();
        let e = take_engine(false);
        let w = Worker::new(e.clone());
        let before = e.reader().get().unwrap();
        let mut stale = before.commit.clone();
        stale.command_seq -= 1;
        assert!(matches!(
            w.prepare_close(
                &stale,
                c.latest(),
                id,
                1,
                16,
                0,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Invalid("STALE_COMMIT"))
        ));
        for no in [0, 2, 3, u64::MAX] {
            assert!(matches!(
                w.prepare_close(
                    &before.commit,
                    c.latest(),
                    id,
                    no,
                    16,
                    0,
                    &Never,
                    &observation(&c),
                    NOW
                ),
                Err(Error::Invalid("RETRY_BUDGET_EXHAUSTED"))
            ));
        }
        assert!(matches!(
            w.prepare_close(
                &before.commit,
                c.latest(),
                schema::ZERO,
                1,
                16,
                0,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Invalid("BATCH_NOT_FOUND"))
        ));
        assert!(
            w.prepare_close(
                &before.commit,
                c.latest(),
                id,
                1,
                16,
                0,
                &Never,
                &observation(&c),
                NOW + 6000
            )
            .is_err()
        );
        assert!(matches!(
            w.prepare_close(
                &before.commit,
                c.snapshot(),
                id,
                1,
                16,
                0,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Invalid("SNAPSHOT_CONFLICT"))
        ));
        assert!(matches!(
            w.prepare_settle(c.latest(), id, 1, 16, 0, &Never, &observation(&c), NOW),
            Err(Error::Invalid("BATCH_STATE"))
        ));
        assert_eq!(before.commit, e.reader().get().unwrap().commit);
        assert_eq!(before.state, e.reader().get().unwrap().state);
        drop(w);
        drop(e);
        let (c, a) = failure_recovery::prepared(bps);
        let e = take_engine(false);
        let w = Worker::new(e.clone());
        let v = e.reader().get().unwrap();
        assert!(matches!(
            w.prepare_close(
                &v.commit,
                c.latest(),
                a["batch"]["batch_id"].as_str().unwrap(),
                1,
                16,
                0,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Invalid("BATCH_STATE"))
        ));
    }
}
#[test]
fn close_original_raw_missing_or_corrupted_closes_before_signing() {
    for mode in ["missing", "corrupt"] {
        let (c, original) = rejected(0);
        let id = original["batch"]["batch_id"].as_str().unwrap();
        let e = take_engine(false);
        let w = Worker::new(e.clone());
        let before = e.reader().get().unwrap();
        let r = e
            .trusted_recovery_failure(&before.commit, id)
            .unwrap()
            .unwrap();
        TRACE.with_borrow(|t| {
            let path = t
                .as_ref()
                .unwrap()
                .dev
                .home
                .join("objects/sha256")
                .join(r.resolution_evidence_ref["sha256"].as_str().unwrap());
            if mode == "missing" {
                std::fs::remove_file(path).unwrap();
            } else {
                std::fs::write(path, b"{}").unwrap();
            }
        });
        assert!(
            w.prepare_close(
                &before.commit,
                c.latest(),
                id,
                1,
                16,
                0,
                &Never,
                &observation(&c),
                NOW
            )
            .is_err()
        );
        assert_eq!(e.reader().get().unwrap().gate, "RECOVERY_REQUIRED");
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
        assert!(matches!(
            w.prepare_close(
                &before.commit,
                c.latest(),
                id,
                1,
                16,
                0,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Recovery("RECOVERY_REQUIRED"))
        ));
    }
}
#[test]
fn close_store_failure_never_broadcasts_and_response_loss_replays() {
    for point in ["candidate_verified", "before_response"] {
        let (mut c, original) = rejected(0);
        let expected = attempt(&mut c, "CLOSE", Some(&original));
        let id = original["batch"]["batch_id"].as_str().unwrap();
        let hash = expected["tx_hash"].as_str().unwrap();
        let e = take_engine(false);
        let w = Worker::new(e.clone());
        let before = e.reader().get().unwrap();
        e.set_fault_hook(Some(Arc::new(move |p| {
            if p == point {
                Err(Error::Recovery("TEST_CLOSE_WRITE"))
            } else {
                Ok(())
            }
        })))
        .unwrap();
        let signer = Signing::new();
        assert!(
            w.prepare_close(
                &before.commit,
                c.latest(),
                id,
                1,
                16,
                0,
                &signer,
                &observation(&c),
                NOW
            )
            .is_err()
        );
        assert_eq!(signer.calls.get(), 1);
        let mut callbacks = 0;
        assert!(
            w.test_broadcast(hash, &observation(&c), NOW, |_, _| callbacks += 1)
                .is_err()
        );
        assert_eq!(callbacks, 0);
        assert_eq!(e.reader().get().unwrap().gate, "RECOVERY_REQUIRED");
        if point == "candidate_verified" {
            assert_eq!(before.commit, e.reader().get().unwrap().commit);
        }
        drop(w);
        drop(e);
        TRACE.with_borrow(|t| {
            let t = t.as_ref().unwrap();
            let disk = failure_recovery::files(&t.dev.home);
            for _ in 0..2 {
                let opened = DevEngine::open(&t.dev.home, t.dev.config.clone());
                if point == "before_response" {
                    let e = opened.unwrap();
                    let v = e.reader().get().unwrap();
                    assert_eq!(v.commit.command_seq, before.commit.command_seq + 1);
                    assert_eq!(
                        e.trusted_recovery_attempt(&v.commit, hash)
                            .unwrap()
                            .unwrap()
                            .attempt,
                        expected
                    );
                } else {
                    assert!(matches!(
                        opened,
                        Err(Error::Recovery("UNKNOWN_OR_INCOMPLETE_STORE"))
                    ));
                }
                assert_eq!(failure_recovery::files(&t.dev.home), disk);
            }
        });
        dev_fixture::evidence(
            &format!("close-write-{point}"),
            &json!({"result":"PASS","signer_calls":1,"callbacks":0,"open_checks":2,"replay_result":if point=="before_response" {"COMMITTED_EXACT"}else{"RECOVERY_REQUIRED_PRESERVED"}}),
        );
    }
}
#[test]
fn close_two_attempt_budget_and_terminal_only_retry() {
    for bps in [0, 25] {
        let (mut c, original) = rejected(bps);
        for no in 1..=2 {
            let mut a = attempt(&mut c, "CLOSE", Some(&original));
            a["attempt_no"] = json!(no.to_string());
            c = replay_check(&c, c.prepare_attempt(a.clone()).unwrap(), "ATTEMPT");
            let next = next_snapshot(&c);
            c = observe(c, next);
            a["state"] = json!("INCLUDED_FAILURE");
            a["confirmed_tx"] = proof(&mut c, &a, "1019");
            c = replay_check(&c, c.resolve_attempt(a).unwrap(), "RESOLVE_ATTEMPT");
        }
        let e = take_engine(true);
        let w = Worker::new(e.clone());
        let before = e.reader().get().unwrap();
        assert!(matches!(
            w.prepare_close(
                &before.commit,
                c.latest(),
                original["batch"]["batch_id"].as_str().unwrap(),
                3,
                16,
                2,
                &Never,
                &observation(&c),
                NOW
            ),
            Err(Error::Invalid("RETRY_BUDGET_EXHAUSTED"))
        ));
        assert_eq!(e.reader().get().unwrap().commit, before.commit);
    }
}
#[test]
fn close_retry_uses_current_snapshot_account_and_original_failure() {
    for bps in [0, 25] {
        let (mut c, original) = rejected(bps);
        let first = attempt(&mut c, "CLOSE", Some(&original));
        c = replay_check(&c, c.prepare_attempt(first.clone()).unwrap(), "ATTEMPT");
        let next = next_snapshot(&c);
        c = observe(c, next);
        let mut terminal = first;
        terminal["state"] = json!("INCLUDED_FAILURE");
        terminal["confirmed_tx"] = proof(&mut c, &terminal, "1019");
        c = replay_check(&c, c.resolve_attempt(terminal).unwrap(), "RESOLVE_ATTEMPT");
        let e = take_engine(true);
        let w = Worker::new(e.clone());
        let before = e.reader().get().unwrap();
        let signer = Signing::new();
        let hash = w
            .prepare_close(
                &before.commit,
                c.latest(),
                original["batch"]["batch_id"].as_str().unwrap(),
                2,
                19,
                7,
                &signer,
                &observation(&c),
                NOW,
            )
            .unwrap();
        let v = e.reader().get().unwrap();
        let r = e
            .trusted_recovery_attempt(&v.commit, &hash)
            .unwrap()
            .unwrap();
        assert_eq!(r.attempt["attempt_no"], "2");
        assert_eq!(r.attempt["account_number"], "19");
        assert_eq!(r.attempt["account_sequence"], "7");
        assert_eq!(
            r.attempt["first_possible_height"],
            (c.latest().height() + 1).to_string()
        );
        let tx = r
            .evidence
            .resolve(&r.attempt["raw_tx_ref"], evidence::TX)
            .unwrap();
        // Independently rebuild the Cosmos sign document with nonzero sequence.
        let owner = schema::bytes(&json!(owner(16))).unwrap();
        use bech32::ToBase32;
        let op = bech32::encode("nus", owner.to_base32(), bech32::Variant::Bech32).unwrap();
        let msg = join(&[
            blob(1, op.as_bytes()),
            blob(
                2,
                c.batch_wire(original["batch"]["batch_id"].as_str().unwrap())
                    .unwrap(),
            ),
            blob(
                3,
                &hex::decode(original["failed_tx_hash"].as_str().unwrap()).unwrap(),
            ),
            blob(
                4,
                &hex::decode(schema::hash("NUS/S3/RESOLUTION_EVIDENCE/V1", &original).unwrap())
                    .unwrap(),
            ),
        ]);
        let body = join(&[
            blob(
                1,
                &join(&[blob(1, b"/nus.exchange.s3.v1.MsgCloseBatch"), blob(2, &msg)]),
            ),
            uint(3, c.latest().height() + 8),
        ]);
        let pkany = join(&[
            blob(1, b"/cosmos.crypto.mldsa65.PubKey"),
            blob(2, &blob(1, &signer.pk)),
        ]);
        let si = join(&[blob(1, &pkany), blob(2, &blob(1, &uint(1, 1))), uint(3, 7)]);
        let fee = join(&[
            blob(1, &join(&[blob(1, b"DEVGAS"), blob(2, b"6000")])),
            uint(2, 3_000_000),
        ]);
        let auth = join(&[blob(1, &si), blob(2, &fee)]);
        let doc = join(&[
            blob(1, &body),
            blob(2, &auth),
            blob(3, b"nus-s3-dev-1"),
            uint(4, 19),
        ]);
        assert_eq!(
            tx,
            join(&[blob(1, &body), blob(2, &auth), blob(3, &sign(16, &doc))])
        );
        assert_eq!(signer.calls.get(), 1);
        assert_eq!(
            e.trusted_recovery_failure(&v.commit, original["batch"]["batch_id"].as_str().unwrap())
                .unwrap()
                .unwrap()
                .raw,
            canonical(&original).unwrap()
        );
        drop(w);
        drop(e);
        check_replays(&v, &hash, &original, "close-retry");
    }
}
#[test]
fn close_wrong_signer_and_commit_race_do_not_persist_attempt() {
    let (c, original) = rejected(0);
    let id = original["batch"]["batch_id"].as_str().unwrap();
    let e = take_engine(false);
    let w = Worker::new(e.clone());
    let before = e.reader().get().unwrap();
    let mut signer = Signing::new();
    signer.pk = hex::decode(key(17)["public_key_hex"].as_str().unwrap()).unwrap();
    assert!(matches!(
        w.prepare_close(
            &before.commit,
            c.latest(),
            id,
            1,
            16,
            0,
            &signer,
            &observation(&c),
            NOW
        ),
        Err(Error::Invalid("FORBIDDEN"))
    ));
    assert_eq!(signer.calls.get(), 0);
    struct Race<'a> {
        signer: Signing,
        e: &'a DevEngine,
        snapshot: Vec<u8>,
        o: Observation,
    }
    impl OperatorSigner for Race<'_> {
        fn public_key(&self) -> &[u8] {
            self.signer.public_key()
        }
        fn sign(&self, doc: &[u8]) -> nus_exchange_contract::s3::dev_local::Result<Vec<u8>> {
            self.e.execute(
                nus_exchange_contract::s3::dev_local::Command::Snapshot(self.snapshot.clone()),
                &[],
                &self.o,
                NOW,
            )?;
            self.signer.sign(doc)
        }
    }
    let race = Race {
        signer: Signing::new(),
        e: &e,
        snapshot: canonical(&finish_snapshot(next_snapshot(&c))).unwrap(),
        o: observation(&c),
    };
    assert!(matches!(
        w.prepare_close(
            &before.commit,
            c.latest(),
            id,
            1,
            16,
            0,
            &race,
            &observation(&c),
            NOW
        ),
        Err(Error::Invalid("STALE_COMMIT"))
    ));
    let after = e.reader().get().unwrap();
    assert_eq!(after.commit.command_seq, before.commit.command_seq + 1);
    assert_eq!(after.state["attempt_refs"], before.state["attempt_refs"]);
}
