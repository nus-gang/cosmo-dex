package keeper

import (
	"bytes"
	"context"
	"encoding/hex"
	"math/big"
	"strconv"

	sdk "github.com/cosmos/cosmos-sdk/types"
	s3 "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/s3types"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

func (k Keeper) batchContext(ctx sdk.Context, b *Batch) error {
	m := b.Body
	if k.ChainID() != S3ChainID || ctx.ChainID() != S3ChainID || ctx.BlockHeight() < 1 || Text(m, "chain_id") != S3ChainID || Text(m, "market_id") != S3Market || Text(m, "genesis_hash") != hex.EncodeToString(k.GenesisHash) || Text(m, "exchange_module_id") != "x/exchange" || Text(m, "market_config_version") != "1" {
		return S3Error("CONTEXT_MISMATCH")
	}
	return nil
}
func (k Keeper) SettleBatch(c context.Context, m *s3.MsgSettleBatch) (*s3.MsgResponse, error) {
	e := k.s3Atomic(sdk.UnwrapSDKContext(c), func(ctx sdk.Context) error {
		b, e := DecodeBatch(m.BatchWire)
		if e != nil {
			return e
		}
		if e = k.batchContext(ctx, b); e != nil {
			return e
		}
		cfg := k.Config(ctx)
		if cfg.Operator != m.Operator {
			return S3Error("OPERATOR_UNAUTHORIZED")
		}
		old, e := k.slot(ctx, b, cfg, false)
		if e != nil {
			return e
		}
		if old != nil {
			ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_s3_retry", sdk.NewAttribute("result", "ALREADY_COMMITTED"), sdk.NewAttribute("original_tx_hash", old.TxHash)))
			return nil
		}
		if e = k.settle(ctx, b, cfg); e != nil {
			return e
		}
		return k.terminal(ctx, b, cfg, nil, nil)
	})
	if e != nil {
		return nil, e
	}
	return &s3.MsgResponse{}, nil
}

type checkedOrder struct {
	body              map[string]any
	record            OrderRecord
	key, owner        string
	max, limit, epoch uint64
}

func (k Keeper) settle(ctx sdk.Context, b *Batch, c S3Config) error {
	orders := map[string]*checkedOrder{}
	owners := map[string]uint64{}
	bindings := map[string]string{}
	// All supplied signatures are checked on every new-effect path, including
	// already bound orders. Cached keys/epochs do not skip any crypto charge.
	for _, v := range b.Orders {
		proof := v.(map[string]any)
		o := proof["order"].(map[string]any)
		wire, e := contract.Encode("OrderV1", o)
		if e != nil {
			return wireError(e)
		}
		if Text(o, "protocol_version") != "1" {
			return S3Error("UNSUPPORTED_VERSION")
		}
		if Text(o, "chain_id") != S3ChainID || Text(o, "genesis_hash") != hex.EncodeToString(k.GenesisHash) || Text(o, "exchange_module_id") != "x/exchange" || Text(o, "market_id") != S3Market || Text(o, "market_config_version") != "1" {
			return S3Error("CONTEXT_MISMATCH")
		}
		pk := Raw(o, "owner_pubkey")
		addr, e := contract.Address(pk)
		if e != nil {
			return wireError(e)
		}
		if !bytes.Equal(addr, Raw(o, "owner")) {
			return S3Error("ADDRESS_MISMATCH")
		}
		owner := sdk.AccAddress(addr).String()
		registered := k.S3Get(ctx, "user/"+owner)
		if registered == nil {
			return S3Error("ACCOUNT_KEY_UNREGISTERED")
		}
		if !bytes.Equal(registered, pk) {
			return S3Error("ACCOUNT_KEY_MISMATCH")
		}
		sig := Raw(proof, "signature")
		ctx.GasMeter().ConsumeGas(750, "S3 embedded ML-DSA-65")
		if !contract.VerifyCrypto(pk, contract.Frame("NUS/ORDER/V1", wire), sig) {
			return S3Error("INVALID_SIGNATURE")
		}
		hash := S3Hash("NUS/ORDER/V1", wire)
		key := orderKey(o)
		orders[hash] = &checkedOrder{o, OrderRecord{hash, wire, sig, "0", false}, key, owner, Number(o, "max_qty_lots"), Number(o, "limit_price_ticks"), Number(o, "owner_epoch")}
	}
	for _, hash := range sortedKeys(orders) {
		o := orders[hash]
		if prior, ok := bindings[o.key]; ok && prior != hash {
			return S3Error("ID_CONFLICT")
		}
		bindings[o.key] = hash
		if prior, ok := k.loadOrder(ctx, o.key); ok {
			if prior.Hash != hash || !bytes.Equal(prior.Wire, o.record.Wire) {
				return S3Error("ID_CONFLICT")
			}
			o.record = prior
		}
	}
	for _, hash := range sortedKeys(orders) {
		o := orders[hash]
		ep, ok := owners[o.owner]
		if !ok {
			ep = k.S3Epoch(ctx, o.owner)
			owners[o.owner] = ep
		}
		if o.epoch != ep {
			return S3Error("EPOCH_MISMATCH")
		}
		if o.record.Revoked {
			return S3Error("ORDER_REVOKED")
		}
		if uint64(ctx.BlockHeight()) >= Number(o.body, "expiry_height") {
			return S3Error("EXPIRED")
		}
		if o.max == 0 || o.max > 1000000 || o.limit == 0 || o.limit > 1000000 || Number(o.body, "side") < 1 || Number(o.body, "side") > 2 || Number(o.body, "order_type") < 1 || Number(o.body, "order_type") > 2 || Text(o.body, "fee_asset_policy_id") != "RECEIVE_ASSET_V1" {
			return S3Error("MARKET_LIMIT")
		}
		if c.FeeBPS > Number(o.body, "max_fee_bps") {
			return S3Error("FEE_CAP")
		}
	}
	type flow struct {
		owner, denom  string
		debit, credit *big.Int
	}
	flows := map[string]*flow{}
	fees := map[string]*big.Int{Base: new(big.Int), Quote: new(big.Int)}
	add := func(owner, denom string, debit, credit *big.Int) {
		key := denom + "/" + owner
		f := flows[key]
		if f == nil {
			f = &flow{owner, denom, new(big.Int), new(big.Int)}
			flows[key] = f
		}
		f.debit.Add(f.debit, debit)
		f.credit.Add(f.credit, credit)
	}
	for _, v := range b.Fills {
		f := v.(map[string]any)
		buy, sell := orders[Text(f, "buyer_order_ref")], orders[Text(f, "seller_order_ref")]
		maker, taker := Text(f, "maker_order_ref"), Text(f, "taker_order_ref")
		if buy == nil || sell == nil || buy == sell || buy.owner == sell.owner || Number(buy.body, "side") != 1 || Number(sell.body, "side") != 2 || !((maker == buy.record.Hash && taker == sell.record.Hash) || (maker == sell.record.Hash && taker == buy.record.Hash)) {
			return S3Error("MARKET_LIMIT")
		}
		q, p := Number(f, "quantity_lots"), Number(f, "execution_price_ticks")
		if q == 0 || q > 1000000 || p == 0 || p > 1000000 || p > buy.limit || p < sell.limit || p != orders[maker].limit || Number(f, "fee_policy_version") != c.FeeVersion {
			return S3Error("MARKET_LIMIT")
		}
		for _, o := range []*checkedOrder{buy, sell} {
			prior, e := Uint(o.record.Filled)
			if e != nil {
				return e
			}
			if prior > o.max || q > o.max-prior {
				return S3Error("CUMULATIVE_QTY_EXCEEDED")
			}
			o.record.Filled = strconv.FormatUint(prior+q, 10)
		}
		if k.S3Get(ctx, "seen/"+Text(f, "fill_id")) != nil {
			return S3Error("DUPLICATE_FILL")
		}
		amounts, e := contract.DevFill(Text(f, "quantity_lots"), Text(f, "execution_price_ticks"), strconv.FormatUint(c.FeeBPS, 10))
		if e != nil {
			return wireError(e)
		}
		base, quote := number128(amounts["base"]), number128(amounts["quote"])
		fb, fq := number128(amounts["fee_base"]), number128(amounts["fee_quote"])
		add(sell.owner, Base, base, new(big.Int))
		add(buy.owner, Quote, quote, new(big.Int))
		add(buy.owner, Base, new(big.Int), new(big.Int).Sub(base, fb))
		add(sell.owner, Quote, new(big.Int), new(big.Int).Sub(quote, fq))
		fees[Base].Add(fees[Base], fb)
		fees[Quote].Add(fees[Quote], fq)
	}
	// Read every C_start and validate all gross debits before writing credits.
	after := map[string]string{}
	for _, key := range sortedKeys(flows) {
		f := flows[key]
		start := "0"
		if raw := k.S3Get(ctx, "a/"+key); raw != nil {
			start = string(raw)
		}
		c0 := number128(start)
		if c0.Cmp(f.debit) < 0 {
			return S3Error("INSUFFICIENT_CONFIRMED_BALANCE")
		}
		result := new(big.Int).Add(new(big.Int).Sub(c0, f.debit), f.credit)
		if e := checked128(result); e != nil {
			return e
		}
		after[key] = result.String()
	}
	for _, d := range []string{Base, Quote} {
		if e := k.ReconcileAsset(ctx, d, true); e != nil {
			return e
		}
	}
	for _, key := range sortedKeys(after) {
		k.S3Set(ctx, "a/"+key, []byte(after[key]))
	}
	for _, d := range []string{Base, Quote} {
		t := k.Total(ctx, d)
		c0 := new(big.Int).Sub(number128(t.Confirmed), fees[d])
		tr := new(big.Int).Add(number128(t.Treasury), fees[d])
		if e := checked128(c0); e != nil {
			return e
		}
		if e := checked128(tr); e != nil {
			return e
		}
		t.Confirmed = c0.String()
		t.Treasury = tr.String()
		k.s3Put(ctx, "total/"+d, t)
	}
	for _, hash := range sortedKeys(orders) {
		o := orders[hash]
		k.saveOrder(ctx, o.key, o.record)
		k.S3Set(ctx, "order_index/"+hash, []byte(o.key))
	}
	for _, v := range b.Fills {
		k.S3Set(ctx, "seen/"+Text(v.(map[string]any), "fill_id"), []byte(Text(b.Body, "batch_seq")))
	}
	return nil
}

func (k Keeper) CloseBatch(c context.Context, m *s3.MsgCloseBatch) (*s3.MsgResponse, error) {
	e := k.s3Atomic(sdk.UnwrapSDKContext(c), func(ctx sdk.Context) error {
		b, e := DecodeBatch(m.BatchWire)
		if e != nil {
			return e
		}
		if e = k.batchContext(ctx, b); e != nil {
			return e
		}
		if len(m.FailedTxHash) != 32 || len(m.ResolutionEvidenceHash) != 32 {
			return S3Error("NON_CANONICAL_WIRE")
		}
		cfg := k.Config(ctx)
		if cfg.Operator != m.Operator {
			return S3Error("OPERATOR_UNAUTHORIZED")
		}
		old, e := k.slot(ctx, b, cfg, true)
		if e != nil {
			return e
		}
		if old != nil {
			result := "ALREADY_CLOSED"
			if old.Disposition == "COMMITTED" {
				result = "ALREADY_COMMITTED"
			}
			ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_s3_retry", sdk.NewAttribute("result", result), sdk.NewAttribute("original_tx_hash", old.TxHash)))
			return nil
		}
		failed, evidence := hex.EncodeToString(m.FailedTxHash), hex.EncodeToString(m.ResolutionEvidenceHash)
		return k.terminal(ctx, b, cfg, &failed, &evidence)
	})
	if e != nil {
		return nil, e
	}
	return &s3.MsgResponse{}, nil
}
