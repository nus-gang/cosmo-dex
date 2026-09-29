use crate::{
    Result,
    codec::{self, Codec, integer},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use serde_json::Value;

pub fn checked_product(a: u128, b: u128, c: u128) -> Result<u128> {
    let ab = BigUint::from(a) * BigUint::from(b);
    let n = ab * BigUint::from(c);
    if n.bits() > 256 {
        return Err("INTERMEDIATE_U256_OVERFLOW");
    }
    n.to_u128().ok_or("FINAL_U128_OVERFLOW")
}
pub fn fee(receive: u128, bps: u32) -> Result<u128> {
    if bps > 10000 {
        return Err("BPS_RANGE");
    }
    let n = (BigUint::from(receive) * BigUint::from(bps) + BigUint::from(9999u32))
        / BigUint::from(10000u32);
    let fee = n.to_u128().ok_or("INTEGER_RANGE")?;
    if fee >= receive {
        return Err("FEE_GE_RECEIVE");
    }
    Ok(fee)
}
pub fn expiry(height: u64, end: u64) -> Result<()> {
    if height >= end {
        Err("EXPIRED")
    } else {
        Ok(())
    }
}
pub fn cumulative(filled: u64, delta: u64, max: u64) -> Result<u64> {
    filled
        .checked_add(delta)
        .filter(|n| *n <= max)
        .ok_or("CUMULATIVE_QTY_EXCEEDED")
}
pub fn fill(q: u64, p: u64, bps: u32) -> Result<(u128, u128, u128, u128)> {
    if !(1..=1000000).contains(&q) || !(1..=1000000).contains(&p) {
        return Err("MARKET_LIMIT");
    }
    let b = checked_product(q as u128, 1000, 1)?;
    let v = checked_product(q as u128, p as u128, 1)?;
    Ok((b, v, fee(b, bps)?, fee(v, bps)?))
}

/// Values must originate from the confirmed account/context snapshot, never the request.
pub struct OrderContext<'a> {
    pub chain_id: &'a str,
    pub genesis_hash: &'a str,
    pub exchange_module_id: &'a str,
    pub market_id: &'a str,
    pub market_config_version: u64,
    pub registered_key: Option<&'a [u8]>,
    pub epoch: u64,
    pub height: u64,
    pub revoked: bool,
    pub filled: u64,
    pub available: u128,
    pub fee_bps: u32,
}
pub fn validate_order(raw: &[u8], sig: &[u8], ctx: &OrderContext<'_>) -> Result<Value> {
    let o = Codec::default().decode("OrderV1", raw)?;
    if o["protocol_version"] != "1" {
        return Err("UNSUPPORTED_VERSION");
    }
    for (k, v) in [
        ("chain_id", ctx.chain_id),
        ("genesis_hash", ctx.genesis_hash),
        ("exchange_module_id", ctx.exchange_module_id),
        ("market_id", ctx.market_id),
    ] {
        if o[k] != v {
            return Err("CONTEXT_MISMATCH");
        }
    }
    if integer(&o["market_config_version"], 64)? != ctx.market_config_version as u128 {
        return Err("CONTEXT_MISMATCH");
    }
    let pk = STANDARD
        .decode(o["owner_pubkey"].as_str().unwrap())
        .map_err(|_| "KEY_LENGTH")?;
    if sig.len() != 3309 {
        return Err("KEY_LENGTH");
    }
    let owner = STANDARD
        .decode(o["owner"].as_str().unwrap())
        .map_err(|_| "ADDRESS_MISMATCH")?;
    if owner != codec::address(&pk)? {
        return Err("ADDRESS_MISMATCH");
    }
    let registered = ctx.registered_key.ok_or("ACCOUNT_KEY_UNREGISTERED")?;
    if registered != pk {
        return Err("ACCOUNT_KEY_MISMATCH");
    }
    if !codec::verify_raw(&pk, &codec::frame("NUS/ORDER/V1", raw), sig, &[]) {
        return Err("INVALID_SIGNATURE");
    }
    if integer(&o["owner_epoch"], 64)? != ctx.epoch as u128 {
        return Err("EPOCH_MISMATCH");
    }
    if ctx.revoked {
        return Err("ORDER_REVOKED");
    }
    expiry(ctx.height, integer(&o["expiry_height"], 64)? as u64)?;
    let q = integer(&o["max_qty_lots"], 64)? as u64;
    let p = integer(&o["limit_price_ticks"], 64)? as u64;
    if !(1..=1000000).contains(&q)
        || !(1..=1000000).contains(&p)
        || !["1", "2"].contains(&o["side"].as_str().unwrap())
        || !["1", "2"].contains(&o["order_type"].as_str().unwrap())
        || o["fee_asset_policy_id"] != "RECEIVE_ASSET_V1"
    {
        return Err("MARKET_LIMIT");
    }
    let cap = integer(&o["max_fee_bps"], 32)?;
    if cap > 10000 || ctx.fee_bps > 10000 || ctx.fee_bps as u128 > cap {
        return Err("FEE_CAP");
    }
    let (base, quote, _, _) = fill(q, p, ctx.fee_bps)?;
    cumulative(ctx.filled, 0, q)?;
    let debit = if o["side"] == "1" { quote } else { base };
    if debit > ctx.available {
        return Err("INSUFFICIENT_CONFIRMED_BALANCE");
    }
    Ok(o)
}
pub fn wallet_policy(
    issued: u64,
    end: u64,
    now: u64,
    origin: &str,
    allowed: &str,
    audience: &str,
    consumed: bool,
) -> bool {
    !consumed
        && issued <= now
        && now < end
        && end.checked_sub(issued).is_some_and(|n| n > 0 && n <= 120)
        && origin == allowed
        && origin == "https://wallet.invalid"
        && ["exchange-api", "private-ws"].contains(&audience)
}
