package keeper

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"strconv"

	sdkmath "cosmossdk.io/math"
	"github.com/cosmos/cosmos-sdk/codec"
	storetypes "github.com/cosmos/cosmos-sdk/store/v2/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	bankkeeper "github.com/cosmos/cosmos-sdk/x/bank/keeper"
	"github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
)

const Module = "exchange"
const Base = "DEVBASE"
const S2ChainID = "nus-s2-dev-1"
const Quote = "DEVQUOTE"
const Gas = "DEVGAS"
const ChainID = "nus-s1-dev-1"

type Keeper struct {
	Key         *storetypes.KVStoreKey
	Bank        bankkeeper.BaseKeeper
	Codec       codec.Codec
	GenesisHash []byte
	Network     string
	S3Binding   *S3Binding
}
type Position struct {
	Amount string `json:"exchange_atoms"`
	Epoch  string `json:"epoch"`
}
type Receipt struct {
	ChainID     string `json:"chain_id"`
	GenesisHash string `json:"genesis_hash"`
	Owner       string `json:"owner"`
	RequestID   string `json:"request_id"`
	RequestHash string `json:"request_hash"`
	Operation   string `json:"operation"`
	Denom       string `json:"denom"`
	Amount      string `json:"amount_atoms"`
	Height      string `json:"committed_height"`
	TxHash      string `json:"original_tx_hash"`
	Before      string `json:"epoch_before"`
	After       string `json:"epoch_after"`
	State       string `json:"state"`
}

func put(s storetypes.KVStore, k []byte, v any) {
	b, e := json.Marshal(v)
	if e != nil {
		panic(e)
	}
	s.Set(k, b)
}
func (k Keeper) ChainID() string {
	if k.Network == "" {
		return ChainID
	}
	return k.Network
}
func (k Keeper) Assets() []string {
	if k.ChainID() == S2ChainID || k.ChainID() == S3ChainID {
		return []string{Base, Quote}
	}
	return []string{Quote}
}
func (k Keeper) Position(ctx sdk.Context, owner string) Position {
	return k.AssetPosition(ctx, owner, Quote)
}
func (k Keeper) AssetPosition(ctx sdk.Context, owner, denom string) Position {
	if k.ChainID() == S3ChainID {
		return k.s3Position(ctx, owner, denom)
	}
	if k.ChainID() == S2ChainID {
		store := ctx.KVStore(k.Key)
		amount, epoch := "0", "0"
		if b := store.Get([]byte("a/" + denom + "/" + owner)); b != nil {
			amount = string(b)
		}
		if b := store.Get([]byte("e/" + owner)); b != nil {
			epoch = string(b)
		}
		return Position{amount, epoch}
	}
	p := Position{"0", "0"}
	b := ctx.KVStore(k.Key).Get([]byte("p/" + owner))
	if b != nil {
		if e := json.Unmarshal(b, &p); e != nil {
			panic(e)
		}
	}
	return p
}
func (k Keeper) Receipt(ctx sdk.Context, owner, id string) (Receipt, bool) {
	var r Receipt
	b := ctx.KVStore(k.Key).Get([]byte("r/" + owner + "/" + id))
	if b == nil {
		return r, false
	}
	if e := json.Unmarshal(b, &r); e != nil {
		panic(e)
	}
	return r, true
}
func Uint(s string) (uint64, error) {
	if s == "" || (len(s) > 1 && s[0] == '0') {
		return 0, fmt.Errorf("NON_CANONICAL_INPUT")
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			return 0, fmt.Errorf("NON_CANONICAL_INPUT")
		}
	}
	n, e := strconv.ParseUint(s, 10, 64)
	if e != nil {
		return 0, fmt.Errorf("INTEGER_RANGE")
	}
	return n, nil
}
func (k Keeper) apply(ctx sdk.Context, msg sdk.Msg, owner, denom, amount string, id []byte, epoch, expiry string, genesis []byte, withdraw bool) error {
	if ctx.ChainID() != k.ChainID() || !bytes.Equal(genesis, k.GenesisHash) {
		return fmt.Errorf("WRONG_CONTEXT")
	}
	addr, e := sdk.AccAddressFromBech32(owner)
	if e != nil || addr.String() != owner {
		return fmt.Errorf("NON_CANONICAL_INPUT")
	}
	if (denom != Quote && !((k.ChainID() == S2ChainID || k.ChainID() == S3ChainID) && denom == Base)) || len(id) != 32 {
		return fmt.Errorf("NON_CANONICAL_INPUT")
	}
	n, e := Uint(amount)
	if e != nil {
		return e
	}
	if n == 0 || n > 1000000000000 {
		return fmt.Errorf("INTEGER_RANGE")
	}
	ep, e := Uint(epoch)
	if e != nil {
		return e
	}
	ex, e := Uint(expiry)
	if e != nil {
		return e
	}
	body, e := k.Codec.Marshal(msg)
	if e != nil {
		return e
	}
	url := sdk.MsgTypeURL(msg)
	frame := binary.BigEndian.AppendUint32(nil, uint32(len(url)))
	frame = append(frame, []byte(url)...)
	frame = binary.BigEndian.AppendUint64(frame, uint64(len(body)))
	frame = append(frame, body...)
	hash := sha256.Sum256(frame)
	hs := hex.EncodeToString(hash[:])
	ids := hex.EncodeToString(id)
	if r, ok := k.Receipt(ctx, owner, ids); ok {
		if r.RequestHash != hs {
			return fmt.Errorf("ID_CONFLICT")
		}
		ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_retry", sdk.NewAttribute("original_tx_hash", r.TxHash)))
		return nil
	}
	if k.ChainID() == S3ChainID {
		if e := k.ReconcileAsset(ctx, denom, true); e != nil {
			return e
		}
	}
	p := k.AssetPosition(ctx, owner, denom)
	current, e := Uint(p.Epoch)
	if e != nil {
		panic(e)
	}
	if ep != current {
		return fmt.Errorf("EPOCH_MISMATCH")
	}
	if ctx.BlockHeight() < 0 || uint64(ctx.BlockHeight()) >= ex {
		return fmt.Errorf("EXPIRED")
	}
	confirmed, ok := sdkmath.NewIntFromString(p.Amount)
	if !ok {
		panic("corrupt amount")
	}
	amt := sdkmath.NewIntFromUint64(n)
	coins := sdk.NewCoins(sdk.NewCoin(denom, amt))
	before := p.Epoch
	operation := "DEPOSIT"
	if withdraw {
		if current == math.MaxUint64 {
			return fmt.Errorf("INTEGER_RANGE")
		}
		if confirmed.LT(amt) {
			return fmt.Errorf("INSUFFICIENT_CONFIRMED_BALANCE")
		}
		if e = k.Bank.SendCoinsFromModuleToAccount(ctx, Module, addr, coins); e != nil {
			return e
		}
		confirmed = confirmed.Sub(amt)
		p.Epoch = strconv.FormatUint(current+1, 10)
		operation = "WITHDRAW"
	} else {
		if k.Bank.GetBalance(ctx, addr, denom).Amount.LT(amt) {
			return fmt.Errorf("INSUFFICIENT_BANK_BALANCE")
		}
		if confirmed.Add(amt).BigInt().BitLen() > 128 {
			return fmt.Errorf("INTEGER_RANGE")
		}
		if e = k.Bank.SendCoinsFromAccountToModule(ctx, addr, Module, coins); e != nil {
			return e
		}
		confirmed = confirmed.Add(amt)
	}
	p.Amount = confirmed.String()
	if k.ChainID() == S3ChainID {
		if e = k.s3ChangeTotal(ctx, denom, n, withdraw); e != nil {
			return e
		}
		k.S3Set(ctx, "a/"+denom+"/"+owner, []byte(p.Amount))
		k.S3Set(ctx, "e/"+owner, []byte(p.Epoch))
		if withdraw {
			k.S3OwnerEvent(ctx, "WITHDRAW", owner, before, p.Epoch, denom, amount, ids, nil)
		}
	} else if k.ChainID() == S2ChainID {
		ctx.KVStore(k.Key).Set([]byte("a/"+denom+"/"+owner), []byte(p.Amount))
		ctx.KVStore(k.Key).Set([]byte("e/"+owner), []byte(p.Epoch))
	} else {
		put(ctx.KVStore(k.Key), []byte("p/"+owner), p)
	}
	txhash := sha256.Sum256(ctx.TxBytes())
	r := Receipt{k.ChainID(), hex.EncodeToString(genesis), owner, ids, hs, operation, denom, amount, strconv.FormatInt(ctx.BlockHeight(), 10), fmt.Sprintf("%X", txhash), before, p.Epoch, "COMMITTED"}
	if k.ChainID() == S3ChainID {
		r.TxHash = hex.EncodeToString(txhash[:])
	}
	put(ctx.KVStore(k.Key), []byte("r/"+owner+"/"+ids), r)
	if k.ChainID() == S3ChainID {
		e = k.ReconcileAsset(ctx, denom, false)
	} else {
		e = k.Invariant(ctx)
	}
	if e != nil {
		return e
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_receipt", sdk.NewAttribute("owner", owner), sdk.NewAttribute("request_id", ids), sdk.NewAttribute("original_tx_hash", r.TxHash)))
	return nil
}
func (k Keeper) Invariant(ctx sdk.Context) error {
	if k.ChainID() == S3ChainID {
		return k.s3Invariant(ctx)
	}
	if k.ChainID() == S2ChainID {
		return k.s2Invariant(ctx)
	}
	total := sdkmath.ZeroInt()
	it := storetypes.KVStorePrefixIterator(ctx.KVStore(k.Key), []byte("p/"))
	defer it.Close()
	for ; it.Valid(); it.Next() {
		var p Position
		if e := json.Unmarshal(it.Value(), &p); e != nil {
			return e
		}
		v, ok := sdkmath.NewIntFromString(p.Amount)
		if !ok || v.IsNegative() || v.BigInt().BitLen() > 128 {
			return fmt.Errorf("INVALID_POSITION")
		}
		total = total.Add(v)
	}
	if !k.Bank.GetBalance(ctx, authtypes.NewModuleAddress(Module), Quote).Amount.Equal(total) {
		return fmt.Errorf("CUSTODY_INVARIANT")
	}
	if !k.Bank.GetSupply(ctx, Quote).Amount.Equal(sdkmath.NewInt(2000000000000)) {
		return fmt.Errorf("SUPPLY_INVARIANT")
	}
	initialGas, ok := sdkmath.NewIntFromString(string(ctx.KVStore(k.Key).Get([]byte("genesis_gas_supply"))))
	if !ok || !k.Bank.GetSupply(ctx, Gas).Amount.Equal(initialGas) {
		return fmt.Errorf("GAS_SUPPLY_INVARIANT")
	}
	quote, gas := sdkmath.ZeroInt(), sdkmath.ZeroInt()
	k.Bank.IterateAllBalances(ctx, func(_ sdk.AccAddress, c sdk.Coin) bool {
		if c.Denom == Quote {
			quote = quote.Add(c.Amount)
		}
		if c.Denom == Gas {
			gas = gas.Add(c.Amount)
		}
		return false
	})
	if !quote.Equal(k.Bank.GetSupply(ctx, Quote).Amount) || !gas.Equal(k.Bank.GetSupply(ctx, Gas).Amount) {
		return fmt.Errorf("BANK_SUPPLY_INVARIANT")
	}
	return nil
}
func (k Keeper) Deposit(c context.Context, m *types.MsgDeposit) (*types.MsgDepositResponse, error) {
	e := k.apply(sdk.UnwrapSDKContext(c), m, m.Owner, m.Denom, m.AmountAtoms, m.RequestId, m.ExpectedEpoch, m.ExpiryHeight, m.GenesisHash, false)
	if e != nil {
		return nil, e
	}
	return &types.MsgDepositResponse{}, nil
}
func (k Keeper) Withdraw(c context.Context, m *types.MsgWithdraw) (*types.MsgWithdrawResponse, error) {
	e := k.apply(sdk.UnwrapSDKContext(c), m, m.Owner, m.Denom, m.AmountAtoms, m.RequestId, m.ExpectedEpoch, m.ExpiryHeight, m.GenesisHash, true)
	if e != nil {
		return nil, e
	}
	return &types.MsgWithdrawResponse{}, nil
}

// s2Invariant reconciles each asset independently, including all bank accounts.
func (k Keeper) s2Invariant(ctx sdk.Context) error {
	for _, denom := range append(k.Assets(), Gas) {
		bankTotal := sdkmath.ZeroInt()
		k.Bank.IterateAllBalances(ctx, func(_ sdk.AccAddress, coin sdk.Coin) bool {
			if coin.Denom == denom {
				bankTotal = bankTotal.Add(coin.Amount)
			}
			return false
		})
		supply := k.Bank.GetSupply(ctx, denom).Amount
		key := "genesis_supply/" + denom
		if denom == Gas {
			key = "genesis_gas_supply"
		}
		initial, ok := sdkmath.NewIntFromString(string(ctx.KVStore(k.Key).Get([]byte(key))))
		if !ok || !initial.Equal(supply) || !bankTotal.Equal(supply) {
			return fmt.Errorf("SUPPLY_INVARIANT %s", denom)
		}
		if denom == Gas {
			continue
		}
		total := sdkmath.ZeroInt()
		it := storetypes.KVStorePrefixIterator(ctx.KVStore(k.Key), []byte("a/"+denom+"/"))
		for ; it.Valid(); it.Next() {
			n, ok := sdkmath.NewIntFromString(string(it.Value()))
			if !ok || n.IsNegative() || n.BigInt().BitLen() > 128 {
				it.Close()
				return fmt.Errorf("INVALID_POSITION")
			}
			total = total.Add(n)
		}
		it.Close()
		if !total.Equal(k.Bank.GetBalance(ctx, authtypes.NewModuleAddress(Module), denom).Amount) {
			return fmt.Errorf("CUSTODY_INVARIANT %s", denom)
		}
	}
	return nil
}
