//! S0 synthetic policy port. No acknowledgement, ledger or durability implementation.
use crate::{
    Result,
    codec::integer,
    policy::{self, OrderContext},
};
use serde_json::{Value, json};

pub fn active_bps(v: &Value) -> Result<u32> {
    let b = integer(v, 32).map_err(|_| "BPS_RANGE")?;
    if b > 10000 {
        return Err("BPS_RANGE");
    }
    Ok(b as u32)
}
pub fn fee_json(receive: &Value, bps: &Value) -> Result<String> {
    let n = integer(receive, 128)?;
    Ok(policy::fee(n, active_bps(bps)?)?.to_string())
}
pub fn cap_check(cap: &Value, bps: &Value) -> Result<()> {
    let cap = integer(cap, 32)?;
    if u128::from(active_bps(bps)?) > cap {
        return Err("FEE_CAP");
    }
    Ok(())
}
fn policy_code(s: &Value) -> Result<()> {
    for k in ["height", "expiry_height", "q", "p"] {
        integer(&s[k], 64)?;
    }
    integer(&s["cap"], 32)?;
    if s["id_state"] == "CONFLICT" {
        return Err("ID_CONFLICT");
    }
    if s["epoch_matches"] == false {
        return Err("EPOCH_MISMATCH");
    }
    if s["revoked"] == true {
        return Err("ORDER_REVOKED");
    }
    policy::expiry(
        integer(&s["height"], 64)? as u64,
        integer(&s["expiry_height"], 64)? as u64,
    )?;
    let q = integer(&s["q"], 64)? as u64;
    let p = integer(&s["p"], 64)? as u64;
    if !(1..=1000000).contains(&q) || !(1..=1000000).contains(&p) {
        return Err("MARKET_LIMIT");
    }
    cap_check(&s["cap"], &s["active_bps"])?;
    policy::fill(q, p, active_bps(&s["active_bps"])?)?;
    if s["cumulative_ok"] == false {
        return Err("CUMULATIVE_QTY_EXCEEDED");
    }
    if s["confirmed_balance_ok"] == false {
        return Err("INSUFFICIENT_CONFIRMED_BALANCE");
    }
    Ok(())
}
pub fn stage(result: Result<()>) -> Value {
    match result {
        Ok(()) => json!({"status":"PASS","code":"OK"}),
        Err("NOT_CONNECTED") => json!({"status":"NOT_CONNECTED","code":null}),
        Err(e) => json!({"status":"REJECTED","code":e}),
    }
}
/// Policy-only entry point. Caller supplies authentication result; this is not crypto evidence.
pub fn evaluate_snapshot(authentication: Value, s: &Value) -> Value {
    let mut out = json!({"authentication":authentication,"snapshot_policy":{
        "status":"NOT_RUN","code":null,"source":"SYNTHETIC","snapshot_id":s["id"]},
        "ack":"NOT_CONNECTED","wal_replay":"NOT_RUN","ledger":"NOT_CONNECTED"});
    if out["authentication"]["status"] != "PASS" {
        return out;
    }
    let required = [
        "id",
        "source",
        "height",
        "expiry_height",
        "epoch_matches",
        "revoked",
        "id_state",
        "cumulative_ok",
        "confirmed_balance_ok",
        "q",
        "p",
        "active_bps",
        "cap",
    ];
    let connected = required.iter().all(|k| !s[*k].is_null())
        && s["source"] == "SYNTHETIC"
        && s["id"].is_string()
        && ["NEW", "CONFLICT"].contains(&s["id_state"].as_str().unwrap_or(""))
        && [
            "epoch_matches",
            "revoked",
            "cumulative_ok",
            "confirmed_balance_ok",
        ]
        .iter()
        .all(|k| s[*k].is_boolean());
    let result = if connected {
        stage(policy_code(s))
    } else {
        stage(Err("NOT_CONNECTED"))
    };
    out["snapshot_policy"]["status"] = result["status"].clone();
    out["snapshot_policy"]["code"] = result["code"].clone();
    out
}
/// Trusted snapshot must be from the same observation as ctx; never accept it from an order request.
pub fn admit_order(raw: &[u8], sig: &[u8], ctx: &OrderContext<'_>, snapshot: &Value) -> Value {
    let authenticated = policy::authenticate_order(raw, sig, ctx);
    let authentication = stage(authenticated.as_ref().map(|_| ()).map_err(|e| *e));
    let mut s = snapshot.clone();
    if let Ok(order) = authenticated {
        let observed_height = ctx.height.to_string();
        // Bind supplied policy fields to the signed order and authentication observation.
        let matches = [
            ("q", "max_qty_lots"),
            ("p", "limit_price_ticks"),
            ("cap", "max_fee_bps"),
            ("expiry_height", "expiry_height"),
        ]
        .iter()
        .all(|(a, b)| s[*a].is_null() || s[*a] == order[*b])
            && !ctx.snapshot_id.is_empty()
            && (s["id"].is_null() || s["id"] == ctx.snapshot_id)
            && (s["height"].is_null() || s["height"].as_str() == Some(observed_height.as_str()));
        if !matches {
            s = Value::Null;
        }
    }
    evaluate_snapshot(authentication, &s)
}
pub fn api_error(code: &str, height: Option<u64>) -> Option<Value> {
    if ![
        "INTEGER_RANGE",
        "BPS_RANGE",
        "FEE_CAP",
        "FEE_GE_RECEIVE",
        "ACCOUNT_KEY_UNREGISTERED",
        "ACCOUNT_KEY_MISMATCH",
    ]
    .contains(&code)
    {
        return None;
    }
    Some(
        json!({"http_status":400,"body":{"code":code,"retryable":false,"state":"REJECTED","height":height.map(|h|h.to_string())}}),
    )
}
