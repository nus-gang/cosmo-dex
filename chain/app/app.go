package app

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"math"
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
	s3 "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/s3types"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
)

const Version = "s1-dev-1"

type OperatorAccount struct {
	Address  string `json:"address"`
	GasAtoms string `json:"gas_atoms"`
}
type Genesis struct {
	PublicKeys       [][]byte          `json:"public_keys"`
	OperatorAccounts []OperatorAccount `json:"operator_accounts"`
}

// Validate rejects malformed allocation before any SDK state is written.
func (g Genesis) Validate() error {
	if len(g.PublicKeys) != 2 {
		return fmt.Errorf("exactly two test users required")
	}
	if len(g.OperatorAccounts) != 4 {
		return fmt.Errorf("exactly four operator accounts required")
	}
	seen := map[string]bool{}
	seen[authtypes.NewModuleAddress("disabled-authority").String()] = true
	for name := range modulePermissions() {
		seen[authtypes.NewModuleAddress(name).String()] = true
	}
	for _, raw := range g.PublicKeys {
		if len(raw) != 1952 {
			return fmt.Errorf("INVALID_KEY")
		}
		address := sdk.AccAddress((&mldsa65.PubKey{Key: raw}).Address()).String()
		if seen[address] {
			return fmt.Errorf("DUPLICATE_ACCOUNT")
		}
		seen[address] = true
	}
	total := uint64(2000000000)
	for _, op := range g.OperatorAccounts {
		addr, err := sdk.AccAddressFromBech32(op.Address)
		if err != nil || len(addr) != 20 || addr.String() != op.Address {
			return fmt.Errorf("INVALID_OPERATOR_ADDRESS")
		}
		if seen[op.Address] {
			return fmt.Errorf("DUPLICATE_ACCOUNT")
		}
		seen[op.Address] = true
		amount, err := ex.Uint(op.GasAtoms)
		if err != nil || amount == 0 || amount > math.MaxUint64-total {
			return fmt.Errorf("INVALID_OPERATOR_GAS")
		}
		total += amount
	}
	return nil
}

func modulePermissions() map[string][]string {
	return map[string][]string{ex.Module: nil, authtypes.FeeCollectorName: nil}
}

// encoding/json normally accepts repeated fields. Reject ambiguity even when
// repeated field names use different JSON escapes.
func uniqueJSONValue(d *json.Decoder, context string) error {
	token, err := d.Token()
	if err != nil {
		return err
	}
	delim, ok := token.(json.Delim)
	if !ok {
		return nil
	}
	switch delim {
	case '{':
		seen := map[string]bool{}
		for d.More() {
			key, err := d.Token()
			if err != nil {
				return err
			}
			name, ok := key.(string)
			if !ok || seen[name] {
				return fmt.Errorf("DUPLICATE_JSON_FIELD")
			}
			seen[name] = true
			// Struct decoding matches case-insensitively; enforce exact decoded
			// schema names before handing values to encoding/json.
			allowed := context == "genesis" && (name == "public_keys" || name == "operator_accounts") ||
				context == "operator_accounts" && (name == "address" || name == "gas_atoms")
			if !allowed {
				return fmt.Errorf("UNKNOWN_JSON_FIELD: %s", name)
			}
			if err := uniqueJSONValue(d, name); err != nil {
				return err
			}
		}
	case '[':
		for d.More() {
			if err := uniqueJSONValue(d, context); err != nil {
				return err
			}
		}
	default:
		return fmt.Errorf("INVALID_JSON_DELIMITER")
	}
	_, err = d.Token()
	return err
}

func DecodeGenesis(raw []byte) (Genesis, error) {
	var g Genesis
	syntax := json.NewDecoder(bytes.NewReader(raw))
	syntax.UseNumber()
	if err := uniqueJSONValue(syntax, "genesis"); err != nil {
		return g, err
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&g); err != nil {
		return g, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return g, fmt.Errorf("trailing genesis data")
	}
	return g, g.Validate()
}

type App struct {
	*baseapp.BaseApp
	Auth           authkeeper.AccountKeeper
	Bank           bankkeeper.BaseKeeper
	Exchange       ex.Keeper
	Codec          *codec.ProtoCodec
	TxConfig       client.TxConfig
	GenesisHash    []byte
	validateS3Init func(*abci.RequestInitChain) error
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
	s3.RegisterInterfaces(r)
	c := codec.NewProtoCodec(r)
	return c, authtx.NewTxConfig(c, []signing.SignMode{signing.SignMode_SIGN_MODE_DIRECT})
}
func New(db dbm.DB, hash []byte, logger log.Logger) (*App, error) {
	return NewForChain(db, hash, logger, ex.ChainID)
}
func NewForChain(db dbm.DB, hash []byte, logger log.Logger, chainID string) (*App, error) {
	return newForChain(db, hash, logger, chainID, nil)
}

func newForChain(db dbm.DB, hash []byte, logger log.Logger, chainID string, binding *ex.S3Binding) (*App, error) {
	if chainID != ex.ChainID && chainID != ex.S2ChainID && chainID != ex.S3ChainID {
		return nil, fmt.Errorf("WRONG_CHAIN")
	}
	if len(hash) != 32 {
		return nil, fmt.Errorf("genesis hash required")
	}
	c, tx := Encoding()
	decode := func(raw []byte) (sdk.Tx, error) {
		if len(raw) > 16384 && (chainID != ex.S3ChainID || len(raw) > ex.MaxSettleTxBytes) {
			return nil, fmt.Errorf("TX_TOO_LARGE")
		}
		return tx.TxDecoder()(raw)
	}
	b := baseapp.NewBaseApp("nusd", logger, db, decode, baseapp.SetChainID(chainID))
	b.SetVersion(Version)
	if chainID == ex.S3ChainID {
		b.SetVersion("s3-dev-1")
	}
	b.SetInterfaceRegistry(c.InterfaceRegistry())
	b.SetTxEncoder(tx.TxEncoder())
	ak := storetypes.NewKVStoreKey("auth")
	bk := storetypes.NewKVStoreKey("bank")
	ek := storetypes.NewKVStoreKey("exchange")
	ck := storetypes.NewKVStoreKey("consensus")
	b.MountStores(ak, bk, ek, ck)
	authority := authtypes.NewModuleAddress("disabled-authority").String()
	auth := authkeeper.NewAccountKeeper(c, runtime.NewKVStoreService(ak), authtypes.ProtoBaseAccount, modulePermissions(), addresscodec.NewBech32Codec("nus"), "nus", authority)
	bank := bankkeeper.NewBaseKeeper(c, runtime.NewKVStoreService(bk), auth, map[string]bool{authtypes.NewModuleAddress(ex.Module).String(): true}, authority, logger)
	cons := consensuskeeper.NewKeeper(c, runtime.NewKVStoreService(ck), authority, nil)
	b.SetParamStore(cons.ParamsStore)
	a := &App{BaseApp: b, Auth: auth, Bank: bank, Exchange: ex.Keeper{Key: ek, Bank: bank, Codec: c, GenesisHash: bytes.Clone(hash), Network: chainID, S3Binding: binding}, Codec: c, TxConfig: tx, GenesisHash: bytes.Clone(hash)}
	ext.RegisterMsgServer(b.MsgServiceRouter(), a.Exchange)
	s3.RegisterMsgServer(b.MsgServiceRouter(), a.Exchange)
	ext.RegisterQueryServer(b.GRPCQueryRouter(), queryServer{a})
	if chainID == ex.S3ChainID {
		authtypes.RegisterQueryServer(b.GRPCQueryRouter(), authkeeper.NewQueryServer(auth))
		banktypes.RegisterQueryServer(b.GRPCQueryRouter(), bank)
	}
	standard, e := ante.NewAnteHandler(ante.HandlerOptions{AccountKeeper: auth, BankKeeper: bank, SignModeHandler: tx.SignModeHandler()})
	if e != nil {
		return nil, e
	}
	b.SetAnteHandler(func(ctx sdk.Context, t sdk.Tx, sim bool) (sdk.Context, error) {
		if chainID == ex.S3ChainID {
			if _, _, e := envelopeS3(t, len(ctx.TxBytes())); e != nil {
				return ctx, e
			}
			next, e := standard(ctx, t, sim)
			if e != nil {
				return next, e
			}
			return next, a.guardS3(next, t)
		}
		if e := a.guard(ctx, t); e != nil {
			return ctx, e
		}
		return standard(ctx, t, sim)
	})
	b.SetInitChainer(a.init)
	if chainID == ex.S3ChainID {
		a.configureS3()
	}
	if chainID == ex.S2ChainID {
		b.SetEndBlocker(func(ctx sdk.Context) (sdk.EndBlock, error) {
			return sdk.EndBlock{}, a.saveS2Header(ctx)
		})
	}
	if e := b.LoadLatestVersion(); e != nil {
		return nil, e
	}
	if b.LastBlockHeight() > 0 {
		store := b.CommitMultiStore().GetKVStore(ek)
		stored := store.Get([]byte("genesis"))
		if !bytes.Equal(stored, hash) || chainID != string(store.Get([]byte("chain_id"))) {
			return nil, fmt.Errorf("genesis hash differs from persisted state")
		}
		if !bytes.Equal(store.Get([]byte("s3_binding")), binding.Bytes()) {
			return nil, fmt.Errorf("S3_BINDING_MISMATCH")
		}
		if binding != nil {
			var cfg ex.S3Config
			if json.Unmarshal(store.Get(a.Exchange.S3Key("config")), &cfg) != nil || cfg.FeeBPS != binding.FeeBPS || cfg.FeeVersion != 1+binding.FeeBPS/25 {
				return nil, fmt.Errorf("S3_BINDING_MISMATCH")
			}
		}
	}
	return a, nil
}
func (a *App) guard(ctx sdk.Context, t sdk.Tx) error {
	if a.Exchange.ChainID() == ex.S3ChainID {
		return a.guardS3(ctx, t)
	}
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
	if !ctx.KVStore(a.Exchange.Key).Has([]byte("user/" + owner)) {
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
	if req.ChainId != a.Exchange.ChainID() {
		return nil, fmt.Errorf("WRONG_CHAIN")
	}
	if a.Exchange.ChainID() == ex.S3ChainID {
		return a.initS3(ctx, req)
	}
	g, err := DecodeGenesis(req.AppStateBytes)
	if err != nil {
		return nil, err
	}
	accounts := authtypes.GenesisAccounts{}
	balances := []banktypes.Balance{}
	for _, raw := range g.PublicKeys {
		pub := &mldsa65.PubKey{Key: raw}
		addr := sdk.AccAddress(pub.Address())
		accounts = append(accounts, authtypes.NewBaseAccount(addr, pub, uint64(len(accounts)), 0))
		coins := sdk.NewCoins(sdk.NewCoin(ex.Quote, sdkmath.NewInt(1000000000000)), sdk.NewCoin(ex.Gas, sdkmath.NewInt(1000000000)))
		if a.Exchange.ChainID() == ex.S2ChainID {
			coins = coins.Add(sdk.NewCoin(ex.Base, sdkmath.NewInt(1000000000000)))
		}
		balances = append(balances, banktypes.Balance{Address: addr.String(), Coins: coins})
	}
	for _, op := range g.OperatorAccounts {
		addr, _ := sdk.AccAddressFromBech32(op.Address)
		amount, _ := sdkmath.NewIntFromString(op.GasAtoms)
		accounts = append(accounts, authtypes.NewBaseAccount(addr, nil, uint64(len(accounts)), 0))
		balances = append(balances, banktypes.Balance{Address: op.Address, Coins: sdk.NewCoins(sdk.NewCoin(ex.Gas, amount))})
	}
	a.Auth.InitGenesis(ctx, *authtypes.NewGenesisState(authtypes.DefaultParams(), accounts))
	a.Auth.GetModuleAccount(ctx, ex.Module)
	a.Auth.GetModuleAccount(ctx, authtypes.FeeCollectorName)
	bg := banktypes.DefaultGenesisState()
	bg.Balances = balances
	a.Bank.InitGenesis(ctx, bg)
	store := ctx.KVStore(a.Exchange.Key)
	store.Set([]byte("genesis"), a.GenesisHash)
	store.Set([]byte("chain_id"), []byte(a.Exchange.ChainID()))
	if a.Exchange.ChainID() == ex.S2ChainID {
		a.initS2Config(ctx)
	}
	for _, denom := range a.Exchange.Assets() {
		store.Set([]byte("genesis_supply/"+denom), []byte(a.Bank.GetSupply(ctx, denom).Amount.String()))
	}
	store.Set([]byte("genesis_gas_supply"), []byte(a.Bank.GetSupply(ctx, ex.Gas).Amount.String()))
	for _, raw := range g.PublicKeys {
		store.Set([]byte("user/"+sdk.AccAddress((&mldsa65.PubKey{Key: raw}).Address()).String()), []byte{1})
	}
	for _, op := range g.OperatorAccounts {
		store.Set([]byte("operator/"+op.Address), []byte(op.GasAtoms))
	}
	return &abci.ResponseInitChain{Validators: req.Validators}, a.Exchange.Invariant(ctx)
}
func (a *App) snapshot(ctx sdk.Context) (map[string]any, error) {
	if a.Exchange.ChainID() == ex.S3ChainID {
		return a.s3Snapshot(ctx)
	}
	if a.Exchange.ChainID() == ex.S2ChainID {
		return a.s2Snapshot(ctx)
	}
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
	operators := []map[string]any{}
	a.Auth.IterateAccounts(ctx, func(ac sdk.AccountI) bool {
		initial := ctx.KVStore(a.Exchange.Key).Get([]byte("operator/" + ac.GetAddress().String()))
		if initial == nil {
			return false
		}
		operators = append(operators, map[string]any{
			"owner": ac.GetAddress().String(), "account_number": strconv.FormatUint(ac.GetAccountNumber(), 10),
			"sequence": strconv.FormatUint(ac.GetSequence(), 10), "gas_atoms": a.Bank.GetBalance(ctx, ac.GetAddress(), ex.Gas).Amount.String(),
			"bank_atoms":        a.Bank.GetBalance(ctx, ac.GetAddress(), ex.Quote).Amount.String(),
			"initial_gas_atoms": string(initial), "exchange_signer": false,
		})
		return false
	})
	out["operator_accounts"] = operators
	out["genesis_gas_supply"] = string(ctx.KVStore(a.Exchange.Key).Get([]byte("genesis_gas_supply")))
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
