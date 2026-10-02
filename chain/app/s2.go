package app

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"sort"
	"strconv"

	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	sdk "github.com/cosmos/cosmos-sdk/types"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
)

// Approved NUS-36 rc2. Changes require a new reviewed contract/genesis.
const S2ContractHash = "2e103517c344f21c2b97fbe7e977f0e614c32c704b0f54978c5bb1fb3ae6ab0f"
const S2ConfigHash = "70281595d471947a56d9bf8a97553dd388a85b107c215a95cc6f34ea9f5f321f"

func (a *App) initS2Config(ctx sdk.Context) {
	market := map[string]string{
		"market_id": "DEVBASE/DEVQUOTE", "config_version": "1",
		"base_denom": ex.Base, "quote_denom": ex.Quote,
		"base_atoms_per_lot": "1000", "quote_atoms_per_lot_tick": "1",
		"min_qty_lots": "1", "max_qty_lots": "1000000",
		"min_price_ticks": "1", "max_price_ticks": "1000000",
		"max_order_quote_atoms": "1000000000000", "fee_policy_version": "1", "fee_bps": "0",
	}
	raw, err := json.Marshal(market)
	if err != nil {
		panic(err)
	}
	s := ctx.KVStore(a.Exchange.Key)
	s.Set([]byte("s2/market"), raw)
	s.Set([]byte("s2/contract_hash"), []byte(S2ContractHash))
	s.Set([]byte("s2/config_hash"), []byte(S2ConfigHash))
}

// Persist H's identity in H's state. Historical queries must not borrow the
// latest header or mistake the header app_hash for H's post-execution root.
func (a *App) saveS2Header(ctx sdk.Context) error {
	if ctx.BlockHeight() <= 0 || len(ctx.HeaderHash()) != 32 || ctx.BlockTime().UnixMilli() < 0 {
		return fmt.Errorf("INVALID_BLOCK_CONTEXT")
	}
	s := ctx.KVStore(a.Exchange.Key)
	s.Set([]byte("s2/height"), []byte(strconv.FormatInt(ctx.BlockHeight(), 10)))
	s.Set([]byte("s2/block_hash"), []byte(hex.EncodeToString(ctx.HeaderHash())))
	s.Set([]byte("s2/block_time"), []byte(strconv.FormatInt(ctx.BlockTime().UnixMilli(), 10)))
	return a.Exchange.Invariant(ctx)
}

func (a *App) s2Snapshot(ctx sdk.Context) (map[string]any, error) {
	if err := a.Exchange.Invariant(ctx); err != nil {
		return nil, err
	}
	s := ctx.KVStore(a.Exchange.Key)
	h := strconv.FormatInt(ctx.BlockHeight(), 10)
	if h != string(s.Get([]byte("s2/height"))) {
		return nil, fmt.Errorf("SNAPSHOT_UNAVAILABLE")
	}
	var market map[string]string
	if err := json.Unmarshal(s.Get([]byte("s2/market")), &market); err != nil {
		return nil, err
	}
	context := map[string]string{
		"schema_version": "1", "chain_id": a.Exchange.ChainID(),
		"genesis_hash":  hex.EncodeToString(s.Get([]byte("genesis"))),
		"contract_hash": string(s.Get([]byte("s2/contract_hash"))),
		"config_hash":   string(s.Get([]byte("s2/config_hash"))),
		"market_id":     market["market_id"], "market_config_version": market["config_version"],
	}
	accounts := []map[string]any{}
	a.Auth.IterateAccounts(ctx, func(ac sdk.AccountI) bool {
		pub, ok := ac.GetPubKey().(*mldsa65.PubKey)
		if !ok || !s.Has([]byte("user/"+ac.GetAddress().String())) {
			return false
		}
		balances := []map[string]string{}
		for _, denom := range a.Exchange.Assets() {
			balances = append(balances, map[string]string{"denom": denom,
				"bank_atoms":      a.Bank.GetBalance(ctx, ac.GetAddress(), denom).Amount.String(),
				"confirmed_atoms": a.Exchange.AssetPosition(ctx, ac.GetAddress().String(), denom).Amount})
		}
		accounts = append(accounts, map[string]any{
			"owner":           base64.StdEncoding.EncodeToString(ac.GetAddress()),
			"public_key_type": "ML_DSA_65", "public_key": base64.StdEncoding.EncodeToString(pub.Bytes()),
			"account_number": strconv.FormatUint(ac.GetAccountNumber(), 10), "sequence": strconv.FormatUint(ac.GetSequence(), 10),
			"owner_epoch": a.Exchange.Position(ctx, ac.GetAddress().String()).Epoch,
			"gas_atoms":   a.Bank.GetBalance(ctx, ac.GetAddress(), ex.Gas).Amount.String(), "balances": balances,
		})
		return false
	})
	sort.Slice(accounts, func(i, j int) bool {
		left, _ := base64.StdEncoding.DecodeString(accounts[i]["owner"].(string))
		right, _ := base64.StdEncoding.DecodeString(accounts[j]["owner"].(string))
		return bytes.Compare(left, right) < 0
	})
	if len(accounts) != 2 {
		return nil, fmt.Errorf("INVALID_REGISTERED_USERS")
	}
	supplies := []map[string]string{}
	for _, denom := range a.Exchange.Assets() {
		supplies = append(supplies, map[string]string{"denom": denom,
			"module_atoms":         a.Bank.GetBalance(ctx, authtypes.NewModuleAddress(ex.Module), denom).Amount.String(),
			"bank_supply_atoms":    a.Bank.GetSupply(ctx, denom).Amount.String(),
			"genesis_supply_atoms": string(s.Get([]byte("genesis_supply/" + denom)))})
	}
	body := map[string]any{"context": context, "observed_height": h,
		"block_hash":         string(s.Get([]byte("s2/block_hash"))),
		"block_time_unix_ms": string(s.Get([]byte("s2/block_time"))),
		"market":             market, "accounts": accounts, "supplies": supplies}
	// All values are contract-constrained ASCII. encoding/json sorts map keys.
	raw, err := json.Marshal(body)
	if err != nil {
		return nil, err
	}
	domain := []byte("NUS/S2/SNAPSHOT/V1")
	frame := binary.BigEndian.AppendUint32(nil, uint32(len(domain)))
	frame = append(frame, domain...)
	frame = binary.BigEndian.AppendUint64(frame, uint64(len(raw)))
	frame = append(frame, raw...)
	hash := sha256.Sum256(frame)
	return map[string]any{"snapshot_id": hex.EncodeToString(hash[:]), "body": body}, nil
}
