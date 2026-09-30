package app

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strconv"

	"cosmossdk.io/log/v2"
	sdkmath "cosmossdk.io/math"
	abci "github.com/cometbft/cometbft/abci/types"
	dbm "github.com/cosmos/cosmos-db"
	"github.com/cosmos/cosmos-sdk/baseapp"
	"github.com/cosmos/cosmos-sdk/client"
	"github.com/cosmos/cosmos-sdk/codec"
	addresscodec "github.com/cosmos/cosmos-sdk/codec/address"
	codectypes "github.com/cosmos/cosmos-sdk/codec/types"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	"github.com/cosmos/cosmos-sdk/runtime"
	"github.com/cosmos/cosmos-sdk/std"
	storetypes "github.com/cosmos/cosmos-sdk/store/v2/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	txtypes "github.com/cosmos/cosmos-sdk/types/tx"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	"github.com/cosmos/cosmos-sdk/x/auth/ante"
	authkeeper "github.com/cosmos/cosmos-sdk/x/auth/keeper"
	authtx "github.com/cosmos/cosmos-sdk/x/auth/tx"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	bankkeeper "github.com/cosmos/cosmos-sdk/x/bank/keeper"
	banktypes "github.com/cosmos/cosmos-sdk/x/bank/types"
	consensuskeeper "github.com/cosmos/cosmos-sdk/x/consensus/keeper"
	txsigning "github.com/cosmos/cosmos-sdk/x/tx/signing"
	"github.com/cosmos/gogoproto/proto"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
)

const Version = "s1-dev-1"

type Genesis struct {
	PublicKeys [][]byte `json:"public_keys"`
}
type App struct {
	*baseapp.BaseApp
	Auth        authkeeper.AccountKeeper
	Bank        bankkeeper.BaseKeeper
	Exchange    ex.Keeper
	Codec       *codec.ProtoCodec
	TxConfig    client.TxConfig
	GenesisHash []byte
}

func Encoding() (*codec.ProtoCodec, client.TxConfig) {
	sdk.GetConfig().SetBech32PrefixForAccount("nus", "nuspub")
	r, e := codectypes.NewInterfaceRegistryWithOptions(codectypes.InterfaceRegistryOptions{ProtoFiles: proto.HybridResolver, SigningOptions: txsigning.Options{AddressCodec: addresscodec.NewBech32Codec("nus"), ValidatorAddressCodec: addresscodec.NewBech32Codec("nusvaloper")}})
	if e != nil {
		panic(e)
	}
	std.RegisterInterfaces(r)
	authtypes.RegisterInterfaces(r)
	banktypes.RegisterInterfaces(r)
	ext.RegisterInterfaces(r)
	c := codec.NewProtoCodec(r)
	return c, authtx.NewTxConfig(c, []signing.SignMode{signing.SignMode_SIGN_MODE_DIRECT})
}
func New(db dbm.DB, hash []byte, logger log.Logger) (*App, error) {
	if len(hash) != 32 {
		return nil, fmt.Errorf("genesis hash required")
	}
	c, tx := Encoding()
	decode := func(raw []byte) (sdk.Tx, error) {
		if len(raw) > 16384 {
			return nil, fmt.Errorf("TX_TOO_LARGE")
		}
		return tx.TxDecoder()(raw)
	}
	b := baseapp.NewBaseApp("nusd", logger, db, decode, baseapp.SetChainID(ex.ChainID))
	b.SetVersion(Version)
	b.SetInterfaceRegistry(c.InterfaceRegistry())
	b.SetTxEncoder(tx.TxEncoder())
	ak := storetypes.NewKVStoreKey("auth")
	bk := storetypes.NewKVStoreKey("bank")
	ek := storetypes.NewKVStoreKey("exchange")
	ck := storetypes.NewKVStoreKey("consensus")
	b.MountStores(ak, bk, ek, ck)
	authority := authtypes.NewModuleAddress("disabled-authority").String()
	auth := authkeeper.NewAccountKeeper(c, runtime.NewKVStoreService(ak), authtypes.ProtoBaseAccount, map[string][]string{ex.Module: nil, authtypes.FeeCollectorName: nil}, addresscodec.NewBech32Codec("nus"), "nus", authority)
	bank := bankkeeper.NewBaseKeeper(c, runtime.NewKVStoreService(bk), auth, map[string]bool{authtypes.NewModuleAddress(ex.Module).String(): true}, authority, logger)
	cons := consensuskeeper.NewKeeper(c, runtime.NewKVStoreService(ck), authority, nil)
	b.SetParamStore(cons.ParamsStore)
	a := &App{b, auth, bank, ex.Keeper{Key: ek, Bank: bank, Codec: c, GenesisHash: bytes.Clone(hash)}, c, tx, bytes.Clone(hash)}
	ext.RegisterMsgServer(b.MsgServiceRouter(), a.Exchange)
	ext.RegisterQueryServer(b.GRPCQueryRouter(), queryServer{a})
	standard, e := ante.NewAnteHandler(ante.HandlerOptions{AccountKeeper: auth, BankKeeper: bank, SignModeHandler: tx.SignModeHandler()})
	if e != nil {
		return nil, e
	}
	b.SetAnteHandler(func(ctx sdk.Context, t sdk.Tx, sim bool) (sdk.Context, error) {
		if e := a.guard(ctx, t); e != nil {
			return ctx, e
		}
		return standard(ctx, t, sim)
	})
	b.SetInitChainer(a.init)
	if e := b.LoadLatestVersion(); e != nil {
		return nil, e
	}
	if b.LastBlockHeight() > 0 {
		stored := b.CommitMultiStore().GetKVStore(ek).Get([]byte("genesis"))
		if !bytes.Equal(stored, hash) {
			return nil, fmt.Errorf("genesis hash differs from persisted state")
		}
	}
	return a, nil
}
func (a *App) guard(ctx sdk.Context, t sdk.Tx) error {
	if len(ctx.TxBytes()) > 16384 {
		return fmt.Errorf("TX_TOO_LARGE")
	}
	w, ok := t.(interface{ GetProtoTx() *txtypes.Tx })
	if !ok {
		return fmt.Errorf("INVALID_TX")
	}
	p := w.GetProtoTx()
	if p.Body == nil || p.AuthInfo == nil || p.AuthInfo.Fee == nil || len(p.Body.Messages) != 1 || len(p.AuthInfo.SignerInfos) != 1 || len(p.Signatures) != 1 {
		return fmt.Errorf("INVALID_ENVELOPE")
	}
	body := p.Body
	fee := p.AuthInfo.Fee
	s := p.AuthInfo.SignerInfos[0]
	if body.Memo != "" || body.TimeoutHeight != 0 || len(body.ExtensionOptions) > 0 || len(body.NonCriticalExtensionOptions) > 0 || fee.Payer != "" || fee.Granter != "" || p.AuthInfo.Tip != nil || body.Unordered || body.TimeoutTimestamp != nil {
		return fmt.Errorf("UNSUPPORTED_OPTION")
	}
	if s == nil || s.PublicKey == nil || s.PublicKey.TypeUrl != "/cosmos.crypto.mldsa65.PubKey" || s.ModeInfo == nil || s.ModeInfo.GetSingle() == nil || s.ModeInfo.GetSingle().Mode != signing.SignMode_SIGN_MODE_DIRECT {
		return fmt.Errorf("UNAUTHORIZED")
	}
	if len(fee.Amount) != 1 || fee.Amount[0].Denom != ex.Gas || !fee.Amount[0].Amount.IsPositive() || fee.GasLimit == 0 || fee.GasLimit > 10000000 {
		return fmt.Errorf("INVALID_FEE")
	}
	if fee.Amount[0].Amount.LT(sdkmath.NewIntFromUint64((fee.GasLimit + 499) / 500)) {
		return fmt.Errorf("INSUFFICIENT_FEE")
	}
	var pub mldsa65.PubKey
	if e := a.Codec.Unmarshal(s.PublicKey.Value, &pub); e != nil || len(pub.Key) != 1952 {
		return fmt.Errorf("UNAUTHORIZED")
	}
	msgs := t.GetMsgs()
	var owner string
	switch m := msgs[0].(type) {
	case *ext.MsgDeposit:
		owner = m.Owner
	case *ext.MsgWithdraw:
		owner = m.Owner
	default:
		return fmt.Errorf("MESSAGE_DISABLED")
	}
	addr, e := sdk.AccAddressFromBech32(owner)
	if e != nil || !bytes.Equal(addr, pub.Address()) {
		return fmt.Errorf("UNAUTHORIZED")
	}
	account := a.Auth.GetAccount(ctx, addr)
	if account == nil || account.GetPubKey() == nil || !account.GetPubKey().Equals(&pub) {
		return fmt.Errorf("UNREGISTERED_KEY")
	}
	if account.GetSequence() == ^uint64(0) {
		return fmt.Errorf("SEQUENCE_OVERFLOW")
	}
	return nil
}
func (a *App) init(ctx sdk.Context, req *abci.RequestInitChain) (*abci.ResponseInitChain, error) {
	if req.ChainId != ex.ChainID {
		return nil, fmt.Errorf("WRONG_CHAIN")
	}
	var g Genesis
	if e := json.Unmarshal(req.AppStateBytes, &g); e != nil {
		return nil, e
	}
	if len(g.PublicKeys) != 2 {
		return nil, fmt.Errorf("exactly two test users required")
	}
	accounts := authtypes.GenesisAccounts{}
	balances := []banktypes.Balance{}
	seen := map[string]bool{}
	for _, raw := range g.PublicKeys {
		if len(raw) != 1952 {
			return nil, fmt.Errorf("INVALID_KEY")
		}
		pub := &mldsa65.PubKey{Key: raw}
		addr := sdk.AccAddress(pub.Address())
		if seen[addr.String()] {
			return nil, fmt.Errorf("DUPLICATE_ACCOUNT")
		}
		seen[addr.String()] = true
		accounts = append(accounts, authtypes.NewBaseAccount(addr, pub, uint64(len(accounts)), 0))
		balances = append(balances, banktypes.Balance{Address: addr.String(), Coins: sdk.NewCoins(sdk.NewCoin(ex.Quote, sdkmath.NewInt(1000000000000)), sdk.NewCoin(ex.Gas, sdkmath.NewInt(1000000000)))})
	}
	a.Auth.InitGenesis(ctx, *authtypes.NewGenesisState(authtypes.DefaultParams(), accounts))
	a.Auth.GetModuleAccount(ctx, ex.Module)
	a.Auth.GetModuleAccount(ctx, authtypes.FeeCollectorName)
	bg := banktypes.DefaultGenesisState()
	bg.Balances = balances
	a.Bank.InitGenesis(ctx, bg)
	ctx.KVStore(a.Exchange.Key).Set([]byte("genesis"), a.GenesisHash)
	return &abci.ResponseInitChain{Validators: req.Validators}, a.Exchange.Invariant(ctx)
}
func (a *App) snapshot(ctx sdk.Context) (map[string]any, error) {
	out := map[string]any{"observed_height": strconv.FormatInt(ctx.BlockHeight(), 10), "genesis_hash": hex.EncodeToString(a.GenesisHash), "chain_id": ex.ChainID, "state": "COMMITTED"}
	users := []map[string]any{}
	a.Auth.IterateAccounts(ctx, func(ac sdk.AccountI) bool {
		if _, ok := ac.GetPubKey().(*mldsa65.PubKey); !ok {
			return false
		}
		addr := ac.GetAddress()
		p := a.Exchange.Position(ctx, addr.String())
		users = append(users, map[string]any{"owner": addr.String(), "account_number": strconv.FormatUint(ac.GetAccountNumber(), 10), "sequence": strconv.FormatUint(ac.GetSequence(), 10), "public_key": ac.GetPubKey().Bytes(), "bank_atoms": a.Bank.GetBalance(ctx, addr, ex.Quote).Amount.String(), "gas_atoms": a.Bank.GetBalance(ctx, addr, ex.Gas).Amount.String(), "exchange_atoms": p.Amount, "epoch": p.Epoch})
		return false
	})
	out["accounts"] = users
	out["module_atoms"] = a.Bank.GetBalance(ctx, authtypes.NewModuleAddress(ex.Module), ex.Quote).Amount.String()
	out["gas_collector_atoms"] = a.Bank.GetBalance(ctx, authtypes.NewModuleAddress(authtypes.FeeCollectorName), ex.Gas).Amount.String()
	out["quote_supply"] = a.Bank.GetSupply(ctx, ex.Quote).Amount.String()
	out["gas_supply"] = a.Bank.GetSupply(ctx, ex.Gas).Amount.String()
	return out, a.Exchange.Invariant(ctx)
}

type queryServer struct{ a *App }

func (q queryServer) Snapshot(c context.Context, _ *ext.QuerySnapshotRequest) (*ext.QueryJSONResponse, error) {
	v, e := q.a.snapshot(sdk.UnwrapSDKContext(c))
	if e != nil {
		return nil, e
	}
	b, e := json.Marshal(v)
	return &ext.QueryJSONResponse{Json: b}, e
}
func (q queryServer) Receipt(c context.Context, r *ext.QueryReceiptRequest) (*ext.QueryJSONResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)
	v, ok := q.a.Exchange.Receipt(ctx, r.Owner, r.RequestId)
	if !ok {
		return nil, fmt.Errorf("NOT_FOUND_AT_HEIGHT %d", ctx.BlockHeight())
	}
	b, e := json.Marshal(v)
	return &ext.QueryJSONResponse{Json: b}, e
}
