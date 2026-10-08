package app

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"reflect"
	"sort"
	"strconv"
	"strings"

	errorsmod "cosmossdk.io/errors"
	sdkmath "cosmossdk.io/math"
	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	storetypes "github.com/cosmos/cosmos-sdk/store/v2/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
)

func canonicalJSON(v any) ([]byte, error) {
	raw, e := json.Marshal(v)
	if e != nil {
		return nil, e
	}
	var m any
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	if e = d.Decode(&m); e != nil {
		return nil, e
	}
	return json.Marshal(m)
}

// BaseApp starts with an empty in-memory header after LoadLatestVersion. S3
// reads the height-pinned committed header identity from its own durable state,
// so a restarted node can answer before producing its first new block.
func (a *App) CreateQueryContext(height int64, prove bool) (sdk.Context, error) {
	if a.Exchange.ChainID() != ex.S3ChainID {
		return a.BaseApp.CreateQueryContext(height, prove)
	}
	ctx, e := a.BaseApp.CreateQueryContextWithCheckHeader(height, prove, false)
	if e != nil {
		return ctx, e
	}
	return ctx.WithChainID(ex.S3ChainID), nil
}
func (a *App) s3Snapshot(ctx sdk.Context) (map[string]any, error) {
	k := a.Exchange
	h := strconv.FormatInt(ctx.BlockHeight(), 10)
	if ctx.BlockHeight() < 1 || string(k.S3Get(ctx, "height")) != h {
		return nil, fmt.Errorf("SNAPSHOT_UNAVAILABLE")
	}
	if e := k.Invariant(ctx); e != nil {
		return nil, e
	}
	last := k.Last(ctx)
	if last.Seq > 0 {
		if _, e := k.StoredReceipt(ctx, last.Seq, last); e != nil {
			return nil, e
		}
	}
	accounts := []map[string]any{}
	sums := map[string]sdkmath.Int{ex.Base: sdkmath.ZeroInt(), ex.Quote: sdkmath.ZeroInt()}
	var err error
	a.Auth.IterateAccounts(ctx, func(ac sdk.AccountI) bool {
		addr := ac.GetAddress()
		key := k.S3Get(ctx, "user/"+addr.String())
		if key == nil {
			return false
		}
		pk, ok := ac.GetPubKey().(*mldsa65.PubKey)
		if !ok || !bytes.Equal(key, pk.Bytes()) {
			err = ex.S3Error("ACCOUNT_KEY_MISMATCH")
			return true
		}
		assets := []map[string]string{}
		for _, d := range []string{ex.Base, ex.Quote} {
			p := k.AssetPosition(ctx, addr.String(), d)
			n, ok := sdkmath.NewIntFromString(p.Amount)
			if !ok || n.IsNegative() || n.BigInt().BitLen() > 128 {
				err = ex.S3Error("INTEGER_RANGE")
				return true
			}
			sums[d] = sums[d].Add(n)
			assets = append(assets, map[string]string{"denom": d, "bank_atoms": a.Bank.GetBalance(ctx, addr, d).Amount.String(), "confirmed_atoms": p.Amount})
		}
		accounts = append(accounts, map[string]any{"owner": []byte(addr), "key_type": "ML-DSA-65", "public_key": key, "epoch": strconv.FormatUint(k.S3Epoch(ctx, addr.String()), 10), "account_number": strconv.FormatUint(ac.GetAccountNumber(), 10), "sequence": strconv.FormatUint(ac.GetSequence(), 10), "gas_atoms": a.Bank.GetBalance(ctx, addr, ex.Gas).Amount.String(), "assets": assets})
		return false
	})
	if err != nil {
		return nil, err
	}
	if len(accounts) < 2 || len(accounts) > 16 {
		return nil, fmt.Errorf("INVALID_REGISTERED_USERS")
	}
	sort.Slice(accounts, func(i, j int) bool {
		return bytes.Compare(accounts[i]["owner"].([]byte), accounts[j]["owner"].([]byte)) < 0
	})
	bankSums := map[string]sdkmath.Int{ex.Base: sdkmath.ZeroInt(), ex.Quote: sdkmath.ZeroInt(), ex.Gas: sdkmath.ZeroInt()}
	a.Bank.IterateAllBalances(ctx, func(_ sdk.AccAddress, c sdk.Coin) bool {
		prior, ok := bankSums[c.Denom]
		if !ok {
			err = fmt.Errorf("UNKNOWN_ASSET")
			return true
		}
		bankSums[c.Denom] = prior.Add(c.Amount)
		return false
	})
	if err != nil {
		return nil, err
	}
	for _, d := range []string{ex.Base, ex.Quote, ex.Gas} {
		supply := a.Bank.GetSupply(ctx, d).Amount
		if !bankSums[d].Equal(supply) || supply.String() != string(ctx.KVStore(k.Key).Get([]byte("genesis_supply/"+d))) {
			return nil, fmt.Errorf("SUPPLY_INVARIANT")
		}
	}
	totals := []map[string]string{}
	for _, d := range []string{ex.Base, ex.Quote} {
		t := k.Total(ctx, d)
		if sums[d].String() != t.Confirmed {
			return nil, ex.S3Error("ASSET_DEFICIT")
		}
		totals = append(totals, map[string]string{"denom": d, "module_bank_atoms": a.Bank.GetBalance(ctx, authtypes.NewModuleAddress(ex.Module), d).Amount.String(), "sum_confirmed_atoms": t.Confirmed, "treasury_atoms": t.Treasury, "unassigned_atoms": t.Unassigned, "supply_atoms": a.Bank.GetSupply(ctx, d).Amount.String()})
	}
	terminal := []string{}
	events := []map[string]any{}
	for _, kind := range []string{"terminal", "event"} {
		it := storetypes.KVStorePrefixIterator(ctx.KVStore(k.Key), k.S3Key(kind+"/"+h+"/"))
		count := 0
		for ; it.Valid(); it.Next() {
			count++
			if count > 34 {
				it.Close()
				return nil, ex.S3Error("RESOURCE_LIMIT")
			}
			if kind == "terminal" {
				terminal = append(terminal, string(it.Value()))
			} else {
				var event map[string]any
				if e := json.Unmarshal(it.Value(), &event); e != nil {
					it.Close()
					return nil, e
				}
				events = append(events, event)
			}
		}
		it.Close()
	}
	cfg := k.Config(ctx)
	operator, e := sdk.AccAddressFromBech32(cfg.Operator)
	if e != nil {
		return nil, e
	}
	out := map[string]any{"context": k.Context(ctx), "height": h, "block_hash": hex.EncodeToString(k.S3Get(ctx, "block_hash")), "block_time_unix_ms": string(k.S3Get(ctx, "block_time")), "accounts": accounts, "assets": totals, "operator": []byte(operator), "operator_epoch": strconv.FormatUint(cfg.Epoch, 10), "last_batch_seq": strconv.FormatUint(last.Seq, 10), "last_batch_hash": last.Hash, "terminal_batch_seqs": terminal, "owner_events": events}
	raw, e := canonicalJSON(out)
	if e != nil {
		return nil, e
	}
	out["snapshot_id"] = ex.S3Hash("NUS/S3/CHAIN_SNAPSHOT/V1", raw)
	return out, nil
}

// S3 query Data is canonical JSON as specified by SCHEMA.md; it is not wrapped
// in a protobuf request. Each query fixes one committed context/height.
func (a *App) Query(goctx context.Context, req *abci.RequestQuery) (*abci.ResponseQuery, error) {
	const prefix = "/nus.exchange.s3.v1.Query/"
	if !strings.HasPrefix(req.Path, prefix) {
		return a.BaseApp.Query(goctx, req)
	}
	response := &abci.ResponseQuery{}
	fail := func(e error) (*abci.ResponseQuery, error) {
		response.Codespace, response.Code, response.Log = errorsmod.ABCIInfo(e, false)
		return response, nil
	}
	if a.Exchange.ChainID() != ex.S3ChainID {
		return fail(ex.S3Error("CONTEXT_MISMATCH"))
	}
	if req.Prove {
		return fail(ex.S3Error("UNSUPPORTED_OPTION"))
	}
	if len(req.Data) > 16384 {
		return fail(ex.S3Error("RESOURCE_LIMIT"))
	}
	var input map[string]json.RawMessage
	if e := strictJSON(req.Data, &input); e != nil {
		return fail(ex.S3Error("NON_CANONICAL_WIRE"))
	}
	raw, e := canonicalJSON(input)
	if e != nil || !bytes.Equal(raw, req.Data) {
		return fail(ex.S3Error("NON_CANONICAL_WIRE"))
	}
	method := strings.TrimPrefix(req.Path, prefix)
	extra := ""
	switch method {
	case "Snapshot":
	case "Batch":
		extra = "batch_seq"
	case "Order":
		extra = "order_hash"
	default:
		return fail(ex.S3Error("UNSUPPORTED_OPTION"))
	}
	fields := 2
	if extra != "" {
		fields++
	}
	if len(input) != fields || input["context"] == nil || input["height"] == nil || extra != "" && input[extra] == nil {
		return fail(ex.S3Error("NON_CANONICAL_WIRE"))
	}
	var height string
	if json.Unmarshal(input["height"], &height) != nil {
		return fail(ex.S3Error("INTEGER_RANGE"))
	}
	n, e := ex.Uint(height)
	if e != nil || n > uint64(^uint64(0)>>1) || int64(n) != req.Height {
		return fail(ex.S3Error("INTEGER_RANGE"))
	}
	ctx, e := a.CreateQueryContext(req.Height, false)
	if e != nil {
		return fail(e)
	}
	response.Height = ctx.BlockHeight()
	var context map[string]string
	if json.Unmarshal(input["context"], &context) != nil || !reflect.DeepEqual(context, a.Exchange.Context(ctx)) {
		return fail(ex.S3Error("CONTEXT_MISMATCH"))
	}
	var out any
	switch method {
	case "Snapshot":
		out, e = a.s3Snapshot(ctx)
	case "Batch":
		var seqText string
		if json.Unmarshal(input[extra], &seqText) != nil {
			return fail(ex.S3Error("INTEGER_RANGE"))
		}
		seq, err := ex.Uint(seqText)
		if err != nil || seq == 0 {
			return fail(ex.S3Error("INTEGER_RANGE"))
		}
		snapshot, err := a.s3Snapshot(ctx)
		if err != nil {
			return fail(err)
		}
		last := a.Exchange.Last(ctx)
		r, err := a.Exchange.StoredReceipt(ctx, seq, last)
		if err != nil {
			return fail(err)
		}
		status := "FOUND"
		if r == nil {
			status = "NOT_FOUND_AT_HEIGHT"
		}
		out = map[string]any{"context": context, "observed_height": strconv.FormatInt(ctx.BlockHeight(), 10), "snapshot_id": snapshot["snapshot_id"], "requested_seq": seqText, "last_seq": strconv.FormatUint(last.Seq, 10), "last_hash": last.Hash, "status": status, "receipt": r}
	case "Order":
		var hash string
		if json.Unmarshal(input[extra], &hash) != nil {
			return fail(ex.S3Error("NON_CANONICAL_WIRE"))
		}
		decoded, err := hex.DecodeString(hash)
		if err != nil || len(decoded) != 32 || hex.EncodeToString(decoded) != hash {
			return fail(ex.S3Error("NON_CANONICAL_WIRE"))
		}
		record, err := a.Exchange.Order(ctx, hash)
		if err != nil {
			return fail(err)
		}
		out = map[string]any{"context": context, "observed_height": strconv.FormatInt(ctx.BlockHeight(), 10), "order_hash": hash, "order": record}
	}
	if e != nil {
		return fail(e)
	}
	response.Value, e = canonicalJSON(out)
	if e != nil {
		return fail(e)
	}
	return response, nil
}
