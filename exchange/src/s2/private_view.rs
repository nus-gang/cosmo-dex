//! Owner-filtered components for LedgerView. The transport supplies the verified
//! session owner; this module does not authenticate an arbitrary owner string.
use super::{
    journal::{canonical, sha256},
    sequencer::Candidate,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, &'static str>;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    owner: String,
    context_hash: String,
    seq: u64,
    orders: usize,
    fills: usize,
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
            if raw.len() > 4096 {
                return Err("SNAPSHOT_CONFLICT");
            }
            let bytes = STANDARD.decode(raw).map_err(|_| "SNAPSHOT_CONFLICT")?;
            let c: Cursor = serde_json::from_slice(&bytes).map_err(|_| "SNAPSHOT_CONFLICT")?;
            if c.owner != owner || c.context_hash != context_hash || c.seq != state.sequence() {
                return Err("SNAPSHOT_CONFLICT");
            }
            (c.orders, c.fills)
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
        STANDARD.encode(
            serde_json::to_vec(&Cursor {
                owner: owner.into(),
                context_hash,
                seq: state.sequence(),
                orders: order_end,
                fills: fill_end,
            })
            .map_err(|_| "STATE_CANONICAL")?,
        )
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
