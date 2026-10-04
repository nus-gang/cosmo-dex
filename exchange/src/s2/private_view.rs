//! Owner-filtered components for LedgerView. The transport supplies the verified
//! session owner; this module does not authenticate an arbitrary owner string.
use super::{
    journal::{canonical, sha256},
    sequencer::Candidate,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, &'static str>;
// 24 bytes of seq/offsets + 32 bytes binding digest, encoded as 75 URL-safe
// characters. This obeys Text <=128 and avoids escaped query/base64 padding.
fn cursor_bytes(seq: u64, orders: u64, fills: u64, owner: &str, context_hash: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(56);
    for n in [seq, orders, fills] {
        bytes.extend(n.to_be_bytes());
    }
    let mut bound = bytes.clone();
    bound.extend_from_slice(owner.as_bytes());
    bound.extend_from_slice(context_hash.as_bytes());
    bytes.extend(hex::decode(sha256(&bound)).expect("sha256 hex"));
    bytes
}

/// A page excludes Status: the service must add live admission status when it
/// builds the final LedgerView. Never send internal EngineState to the client.
pub fn page(
    state: &Candidate,
    owner: &str,
    cursor: Option<&str>,
    order_limit: usize,
    fill_limit: usize,
) -> Result<Value> {
    if !(1..=200).contains(&order_limit) || !(1..=1000).contains(&fill_limit) {
        return Err("PAGE_LIMIT");
    }
    let account = state
        .snapshot()
        .accounts()
        .iter()
        .find(|a| a.owner == owner)
        .ok_or("ACCOUNT_KEY_UNREGISTERED")?;
    let context = &state.snapshot().value()["body"]["context"];
    let context_hash = sha256(&canonical(context).map_err(|_| "STATE_CANONICAL")?);
    let (order_offset, fill_offset) = match cursor {
        None => (0, 0),
        Some(raw) => {
            if raw.len() != 75 {
                return Err("SNAPSHOT_CONFLICT");
            }
            let bytes = URL_SAFE_NO_PAD
                .decode(raw)
                .map_err(|_| "SNAPSHOT_CONFLICT")?;
            if bytes.len() != 56 || URL_SAFE_NO_PAD.encode(&bytes) != raw {
                return Err("SNAPSHOT_CONFLICT");
            }
            let seq = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
            let orders = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
            let fills = u64::from_be_bytes(bytes[16..24].try_into().unwrap());
            if seq != state.sequence()
                || bytes != cursor_bytes(seq, orders, fills, owner, &context_hash)
            {
                return Err("SNAPSHOT_CONFLICT");
            }
            (
                usize::try_from(orders).map_err(|_| "SNAPSHOT_CONFLICT")?,
                usize::try_from(fills).map_err(|_| "SNAPSHOT_CONFLICT")?,
            )
        }
    };
    // Reuse the validated internal projection but explicitly whitelist outgoing
    // fields. Signed evidence, counterparties and internal debits never escape.
    let internal = state.state_json("OPEN")?;
    let orders: Vec<_> = internal["orders"]
        .as_array()
        .ok_or("STATE_CANONICAL")?
        .iter()
        .filter(|o| o["owner"] == owner)
        .map(|o| o["view"].clone())
        .collect();
    let mut fills = Vec::new();
    for fill in internal["fills"].as_array().ok_or("STATE_CANONICAL")? {
        let own = ["buyer_order_hash", "seller_order_hash"]
            .into_iter()
            .filter_map(|key| fill[key].as_str().and_then(|hash| state.orders().get(hash)))
            .find(|order| order.live.owner == owner);
        let Some(own) = own else { continue };
        let mut view = json!({"own_order_id":own.order_id});
        for key in [
            "fill_id",
            "command_seq",
            "match_index",
            "quantity_lots",
            "execution_price_ticks",
            "fee_policy_version",
            "fee_base_atoms",
            "fee_quote_atoms",
            "state",
            "reason",
            "revision",
        ] {
            view[key] = fill[key].clone();
        }
        fills.push(view);
    }
    if order_offset > orders.len() || fill_offset > fills.len() {
        return Err("SNAPSHOT_CONFLICT");
    }
    let order_end = orders.len().min(order_offset.saturating_add(order_limit));
    let fill_end = fills.len().min(fill_offset.saturating_add(fill_limit));
    let next = if order_end == orders.len() && fill_end == fills.len() {
        "END".to_owned()
    } else {
        URL_SAFE_NO_PAD.encode(cursor_bytes(
            state.sequence(),
            order_end as u64,
            fill_end as u64,
            owner,
            &context_hash,
        ))
    };
    let ledger = internal["accounts"]
        .as_array()
        .ok_or("STATE_CANONICAL")?
        .iter()
        .find(|a| a["owner"] == owner)
        .ok_or("LEDGER_RECONCILIATION")?;
    Ok(
        json!({"context":context, "owner":owner, "owner_epoch":account.epoch.to_string(),
        "stream_seq":state.sequence().to_string(), "revision":state.sequence().to_string(),
        "snapshot_id":state.snapshot().id(), "observed_height":state.snapshot().height().to_string(),
        "ledger":ledger["ledger"], "orders":orders[order_offset..order_end],
        "fills":fills[fill_offset..fill_end], "next_cursor":next}),
    )
}
