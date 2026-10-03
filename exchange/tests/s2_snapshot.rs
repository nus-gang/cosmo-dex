use base64::{Engine, engine::general_purpose::STANDARD};
use nus_exchange_contract::{
    codec,
    s2::{
        journal::{canonical, sha256},
        snapshot::{Advance, Binding, Observation, Snapshot},
    },
};
use serde_json::{Value, json};
fn fixture() -> Value {
    serde_json::from_str::<Value>(include_str!("../../protocol/s2/vectors/snapshot.json")).unwrap()
        ["snapshot"]
        .clone()
}
fn binding(v: &Value) -> Binding {
    let body = &v["body"];
    Binding::new(
        body["context"].clone(),
        body["market"].clone(),
        [
            STANDARD
                .decode(body["accounts"][0]["owner"].as_str().unwrap())
                .unwrap(),
            STANDARD
                .decode(body["accounts"][1]["owner"].as_str().unwrap())
                .unwrap(),
        ],
        [2_000_000_000_000; 2],
    )
    .unwrap()
}
fn rehash(v: &mut Value) {
    v["snapshot_id"] = json!(sha256(&codec::frame(
        "NUS/S2/SNAPSHOT/V1",
        &canonical(&v["body"]).unwrap()
    )));
}
fn decode(b: &Binding, v: &Value) -> Result<Snapshot, &'static str> {
    b.decode(&serde_json::to_vec(v).unwrap())
}
#[test]
fn canonical_contract_fixture_and_key_owner_binding() {
    let v = fixture();
    let b = binding(&v);
    let s = decode(&b, &v).unwrap();
    assert_eq!(s.height(), 100);
    assert_eq!(
        s.id(),
        "9ff795d16f77b68bf1092f9487cd5de3e08c6ecde21fb9fab718a2024a27fc3a"
    );
    assert_eq!(s.accounts()[0].confirmed, [0, 100_000_000]);
    assert_eq!(s.accounts()[1].confirmed, [10_000_000, 0]);
    assert_eq!(s.advance(&s), Ok(Advance::Duplicate));
}
#[test]
fn rejects_mixed_context_and_hash_tampering() {
    let original = fixture();
    let b = binding(&original);
    for key in [
        "genesis_hash",
        "contract_hash",
        "config_hash",
        "chain_id",
        "market_id",
        "market_config_version",
    ] {
        let mut v = original.clone();
        v["body"]["context"][key] = json!("different");
        rehash(&mut v);
        assert_eq!(decode(&b, &v), Err("CONTEXT_MISMATCH"), "{key}");
    }
    let mut v = original;
    v["body"]["observed_height"] = json!("101");
    assert_eq!(decode(&b, &v), Err("SNAPSHOT_HASH"));
}
#[test]
fn strict_structure_ranges_base64_and_duplicate_json_keys() {
    let original = fixture();
    let b = binding(&original);
    for pointer in [
        "/body/observed_height",
        "/body/accounts/0/owner_epoch",
        "/body/accounts/0/sequence",
    ] {
        for invalid in [
            json!(0),
            json!("01"),
            json!("18446744073709551616"),
            json!(null),
        ] {
            let mut v = original.clone();
            *v.pointer_mut(pointer).unwrap() = invalid;
            // JSON number is invalid even before semantic validation.
            if canonical(&v["body"]).is_ok() {
                rehash(&mut v);
            }
            assert!(decode(&b, &v).is_err(), "{pointer}");
        }
    }
    let mut v = original.clone();
    v["body"]["accounts"][0]["extra"] = json!(false);
    rehash(&mut v);
    assert_eq!(decode(&b, &v), Err("SNAPSHOT_SCHEMA"));
    let mut v = original.clone();
    v["body"]["accounts"][0]["public_key"] = json!("AAAA");
    rehash(&mut v);
    assert_eq!(decode(&b, &v), Err("SNAPSHOT_BYTES"));
    let raw = serde_json::to_string(&original).unwrap().replacen(
        "\"observed_height\":\"100\"",
        "\"observed_height\":\"99\",\"observed_height\":\"100\"",
        1,
    );
    assert_eq!(b.decode(raw.as_bytes()), Err("NON_CANONICAL_JSON"));
    assert_eq!(b.decode(&vec![b' '; 65537]), Err("RESOURCE_LIMIT"));
}
#[test]
fn registered_accounts_and_asset_order_are_exact() {
    let original = fixture();
    let b = binding(&original);
    for field in ["accounts", "supplies"] {
        let mut v = original.clone();
        v["body"][field].as_array_mut().unwrap().swap(0, 1);
        rehash(&mut v);
        assert!(decode(&b, &v).is_err());
        let mut v = original.clone();
        v["body"][field].as_array_mut().unwrap().pop();
        rehash(&mut v);
        assert_eq!(decode(&b, &v), Err("SNAPSHOT_SCHEMA"));
    }
    let mut v = original.clone();
    v["body"]["accounts"][0]["public_key"] = v["body"]["accounts"][1]["public_key"].clone();
    rehash(&mut v);
    assert_eq!(decode(&b, &v), Err("ACCOUNT_KEY_MISMATCH"));
    let mut v = original;
    v["body"]["accounts"][0]["balances"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    rehash(&mut v);
    assert_eq!(decode(&b, &v), Err("SNAPSHOT_SCHEMA"));
}
#[test]
fn conservation_and_checked_sum_fail_closed() {
    let original = fixture();
    let b = binding(&original);
    for pointer in [
        "/body/accounts/0/balances/0/confirmed_atoms",
        "/body/accounts/1/balances/1/bank_atoms",
        "/body/supplies/0/module_atoms",
        "/body/supplies/1/bank_supply_atoms",
        "/body/supplies/0/genesis_supply_atoms",
    ] {
        let mut v = original.clone();
        *v.pointer_mut(pointer).unwrap() = json!("1");
        rehash(&mut v);
        assert_eq!(decode(&b, &v), Err("ASSET_CONSERVATION"), "{pointer}");
    }
    let mut v = original;
    v["body"]["accounts"][0]["balances"][0]["confirmed_atoms"] = json!(u128::MAX.to_string());
    rehash(&mut v);
    assert_eq!(decode(&b, &v), Err("INTEGER_OVERFLOW"));
}
#[test]
fn cursor_is_contiguous_and_conflicts_never_replace_committed_snapshot() {
    let v = fixture();
    let b = binding(&v);
    let s = decode(&b, &v).unwrap();
    for (height, expected) in [
        ("99", Err("HEIGHT_REGRESSION")),
        ("102", Err("CATCHING_UP")),
        (
            "101",
            Ok(Advance::Next {
                epoch_changed_owners: vec![],
            }),
        ),
    ] {
        let mut next = v.clone();
        next["body"]["observed_height"] = json!(height);
        rehash(&mut next);
        assert_eq!(s.advance(&decode(&b, &next).unwrap()), expected);
        assert_eq!(s.value(), &v);
    }
    let mut next = v.clone();
    next["body"]["block_hash"] = json!("a".repeat(64));
    rehash(&mut next);
    assert_eq!(
        s.advance(&decode(&b, &next).unwrap()),
        Err("SNAPSHOT_CONFLICT")
    );
}
#[test]
fn withdrawal_requires_epoch_advance_and_returns_correction_seed() {
    let v = fixture();
    let b = binding(&v);
    let s = decode(&b, &v).unwrap();
    let mut next = v;
    next["body"]["observed_height"] = json!("101");
    next["body"]["accounts"][0]["balances"][1]["confirmed_atoms"] = json!("99999999");
    next["body"]["accounts"][0]["balances"][1]["bank_atoms"] = json!("999900000001");
    next["body"]["supplies"][1]["module_atoms"] = json!("99999999");
    rehash(&mut next);
    assert_eq!(
        s.advance(&decode(&b, &next).unwrap()),
        Err("UNPROVEN_BALANCE_DECREASE")
    );
    next["body"]["accounts"][0]["owner_epoch"] = json!("1");
    rehash(&mut next);
    let advanced = decode(&b, &next).unwrap();
    assert_eq!(
        s.advance(&advanced),
        Ok(Advance::Next {
            epoch_changed_owners: vec![s.accounts()[0].owner.clone()]
        })
    );
    next["body"]["observed_height"] = json!("102");
    next["body"]["accounts"][0]["owner_epoch"] = json!("0");
    rehash(&mut next);
    assert_eq!(
        advanced.advance(&decode(&b, &next).unwrap()),
        Err("EPOCH_MISMATCH")
    );
}
#[test]
fn freshness_thresholds_use_recorded_times_and_bound_cursor() {
    let v = fixture();
    let b = binding(&v);
    let s = decode(&b, &v).unwrap();
    let t = 1790956680000;
    let o = Observation {
        snapshot_id: s.id().into(),
        cursor_height: 100,
        received_at: t,
        query_latency_ms: 2000,
        catching_up: false,
    };
    assert_eq!(s.freshness(&o, t + 5000), Ok(()));
    assert_eq!(s.freshness(&o, t + 5001), Err("STALE"));
    let mut o = o.clone();
    o.received_at = t + 5001;
    assert_eq!(s.freshness(&o, t + 5001), Err("STALE")); // fresh RPC, stale block
    o.received_at = t - 1000;
    assert_eq!(s.freshness(&o, t - 1000), Ok(()));
    o.received_at = t - 1001;
    assert_eq!(s.freshness(&o, t - 1001), Err("STALE"));
    o.received_at = t;
    o.query_latency_ms = 2001;
    assert_eq!(s.freshness(&o, t), Err("STALE"));
    o.query_latency_ms = 0;
    o.catching_up = true;
    assert_eq!(s.freshness(&o, t), Err("CATCHING_UP"));
    o.catching_up = false;
    o.cursor_height = 99;
    assert_eq!(s.freshness(&o, t), Err("SNAPSHOT_CONFLICT"));
}

#[test]
fn account_identity_sequence_and_block_time_cannot_regress() {
    let v = fixture();
    let b = binding(&v);
    let s = decode(&b, &v).unwrap();
    for (pointer, value) in [
        ("/body/accounts/0/sequence", "0"),
        ("/body/accounts/0/account_number", "5"),
        ("/body/block_time_unix_ms", "1"),
    ] {
        let mut next = v.clone();
        next["body"]["observed_height"] = json!("101");
        *next.pointer_mut(pointer).unwrap() = json!(value);
        rehash(&mut next);
        assert_eq!(
            s.advance(&decode(&b, &next).unwrap()),
            Err("SNAPSHOT_CONFLICT")
        );
    }
    let mut next = v;
    next["body"]["accounts"][1]["account_number"] = json!("1");
    rehash(&mut next);
    assert_eq!(decode(&b, &next), Err("SNAPSHOT_CONFLICT"));
}
