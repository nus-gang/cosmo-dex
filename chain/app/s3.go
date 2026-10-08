package app

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"strconv"

	sdkmath "cosmossdk.io/math"
	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	sdk "github.com/cosmos/cosmos-sdk/types"
	txtypes "github.com/cosmos/cosmos-sdk/types/tx"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	banktypes "github.com/cosmos/cosmos-sdk/x/bank/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	s3 "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/s3types"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
)

// S3 has its own genesis shape. Legacy gas-allocation addresses do not acquire
// operator authority, and no existing S1/S2 database is migrated.
type S3Genesis struct {
	PublicKeys   [][]byte `json:"public_keys"`
	OperatorKeys [][]byte `json:"settlement_operator_public_keys"`
	AdminKey     []byte   `json:"admin_public_key"`
	FeeBPS       string   `json:"fee_bps"`
	ContractHash string   `json:"contract_hash"`
	ConfigHash   string   `json:"config_hash"`
}

// Reject a different local-demo genesis before BaseApp initializes its caches
// and consensus store, so a rejected request cannot poison a later valid init.
func (a *App) InitChain(req *abci.RequestInitChain) (*abci.ResponseInitChain, error) {
	if a.validateS3Init != nil {
		if err := a.validateS3Init(req); err != nil {
			return nil, err
		}
	}
	return a.BaseApp.InitChain(req)
}

func uniqueValue(d *json.Decoder) error {
	v, e := d.Token()
	if e != nil {
		return e
	}
	delim, ok := v.(json.Delim)
	if !ok {
		return nil
	}
	switch delim {
	case '{':
		seen := map[string]bool{}
		for d.More() {
			key, e := d.Token()
			if e != nil {
				return e
			}
			s, ok := key.(string)
			if !ok || seen[s] {
				return fmt.Errorf("DUPLICATE_JSON_FIELD")
			}
			seen[s] = true
			if e = uniqueValue(d); e != nil {
				return e
			}
		}
	case '[':
		for d.More() {
			if e = uniqueValue(d); e != nil {
				return e
			}
		}
	default:
		return fmt.Errorf("INVALID_JSON")
	}
	_, e = d.Token()
	return e
}
func strictJSON(raw []byte, v any) error {
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	if e := uniqueValue(d); e != nil {
		return e
	}
	d = json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if e := d.Decode(v); e != nil {
		return e
	}
	if e := d.Decode(new(any)); e != io.EOF {
		return fmt.Errorf("TRAILING_JSON")
	}
	return nil
}
func DecodeS3Genesis(raw []byte) (S3Genesis, error) {
	return decodeS3Genesis(raw, nil)
}
func decodeS3Genesis(raw []byte, binding *ex.S3Binding) (S3Genesis, error) {
	var g S3Genesis
	if e := strictJSON(raw, &g); e != nil {
		return g, e
	}
	var fields map[string]any
	if e := json.Unmarshal(raw, &fields); e != nil {
		return g, e
	}
	allowed := []string{"public_keys", "settlement_operator_public_keys", "admin_public_key", "fee_bps", "contract_hash", "config_hash"}
	if len(fields) != len(allowed) {
		return g, fmt.Errorf("INVALID_S3_GENESIS")
	}
	for _, key := range allowed {
		if _, ok := fields[key]; !ok {
			return g, fmt.Errorf("INVALID_S3_GENESIS_FIELD")
		}
	}
	if len(g.PublicKeys) < 2 || len(g.PublicKeys) > 16 || len(g.OperatorKeys) != 2 {
		return g, fmt.Errorf("S3 requires 2..16 users and two distinct operator keys")
	}
	config := ex.S3ConfigHash
	if g.FeeBPS == "25" {
		config = ex.S3Fee25ConfigHash
	} else if g.FeeBPS != "0" {
		return g, fmt.Errorf("INVALID_FEE_PROFILE")
	}
	contractHash := ex.S3ContractHash
	if binding != nil {
		if g.FeeBPS != strconv.FormatUint(binding.FeeBPS, 10) {
			return g, fmt.Errorf("INVALID_FEE_PROFILE")
		}
		contractHash, config = binding.ContractHash, binding.ConfigHash
	}
	if g.ContractHash != contractHash || g.ConfigHash != config {
		return g, fmt.Errorf("CONTEXT_MISMATCH")
	}
	seen := map[string]bool{}
	for _, group := range [][][]byte{g.PublicKeys, g.OperatorKeys, {g.AdminKey}} {
		for _, raw := range group {
			if len(raw) != 1952 {
				return g, fmt.Errorf("KEY_LENGTH")
			}
			// Compare raw addresses before app encoding is initialized. Calling
			// AccAddress.String here would cache the process's old Bech32 prefix.
			addr := string((&mldsa65.PubKey{Key: raw}).Address())
			if seen[addr] {
				return g, fmt.Errorf("DUPLICATE_ACCOUNT")
			}
			seen[addr] = true
		}
	}
	return g, nil
}
func (a *App) initS3(ctx sdk.Context, req *abci.RequestInitChain) (*abci.ResponseInitChain, error) {
	if a.validateS3Init != nil {
		if e := a.validateS3Init(req); e != nil {
			return nil, e
		}
	}
	g, e := decodeS3Genesis(req.AppStateBytes, a.Exchange.S3Binding)
	if e != nil {
		return nil, e
	}
	p := req.ConsensusParams
	if req.InitialHeight != 1 || len(req.Validators) != 4 || p == nil || p.Block == nil || p.Evidence == nil || p.Block.MaxBytes != 1048576 || p.Block.MaxGas != 20000000 || p.Evidence.MaxBytes != 65536 {
		return nil, fmt.Errorf("S3_CONSENSUS_PROFILE")
	}
	validators := map[string]bool{}
	for _, v := range req.Validators {
		key := v.PubKey.GetEd25519()
		if len(key) != 32 || v.Power <= 0 || validators[string(key)] {
			return nil, fmt.Errorf("S3_VALIDATOR_PROFILE")
		}
		validators[string(key)] = true
	}
	accounts := authtypes.GenesisAccounts{}
	balances := []banktypes.Balance{}
	for group, keys := range [][][]byte{g.PublicKeys, g.OperatorKeys, {g.AdminKey}} {
		for _, raw := range keys {
			pub := &mldsa65.PubKey{Key: raw}
			addr := sdk.AccAddress(pub.Address())
			accounts = append(accounts, authtypes.NewBaseAccount(addr, pub, uint64(len(accounts)), 0))
			coins := sdk.NewCoins(sdk.NewCoin(ex.Gas, sdkmath.NewInt(1000000000)))
			if group == 0 {
				coins = coins.Add(sdk.NewCoin(ex.Base, sdkmath.NewInt(1000000000000)), sdk.NewCoin(ex.Quote, sdkmath.NewInt(1000000000000)))
			}
			balances = append(balances, banktypes.Balance{Address: addr.String(), Coins: coins})
		}
	}
	a.Auth.InitGenesis(ctx, *authtypes.NewGenesisState(authtypes.DefaultParams(), accounts))
	a.Auth.GetModuleAccount(ctx, ex.Module)
	a.Auth.GetModuleAccount(ctx, authtypes.FeeCollectorName)
	bg := banktypes.DefaultGenesisState()
	bg.Balances = balances
	a.Bank.InitGenesis(ctx, bg)
	s := ctx.KVStore(a.Exchange.Key)
	s.Set([]byte("genesis"), a.GenesisHash)
	s.Set([]byte("chain_id"), []byte(ex.S3ChainID))
	if binding := a.Exchange.S3Binding.Bytes(); binding != nil {
		s.Set([]byte("s3_binding"), binding)
	}
	for _, d := range []string{ex.Base, ex.Quote, ex.Gas} {
		s.Set([]byte("genesis_supply/"+d), []byte(a.Bank.GetSupply(ctx, d).Amount.String()))
	}
	s.Set([]byte("genesis_gas_supply"), []byte(a.Bank.GetSupply(ctx, ex.Gas).Amount.String()))
	bps, _ := ex.Uint(g.FeeBPS)
	version := uint64(1)
	if bps == 25 {
		version = 2
	}
	cfg := ex.S3Config{Operator: sdk.AccAddress((&mldsa65.PubKey{Key: g.OperatorKeys[0]}).Address()).String(), Epoch: 1, Admin: sdk.AccAddress((&mldsa65.PubKey{Key: g.AdminKey}).Address()).String(), FeeBPS: bps, FeeVersion: version}
	a.Exchange.InitS3(ctx, cfg, g.PublicKeys, g.OperatorKeys)
	return &abci.ResponseInitChain{Validators: req.Validators}, a.Exchange.Invariant(ctx)
}

func envelopeS3(t sdk.Tx, rawSize int) (*txtypes.Tx, string, error) {
	w, ok := t.(interface{ GetProtoTx() *txtypes.Tx })
	if !ok {
		return nil, "", ex.S3Error("INVALID_ENVELOPE")
	}
	p := w.GetProtoTx()
	if p.Body == nil || p.AuthInfo == nil || p.AuthInfo.Fee == nil || len(p.Body.Messages) != 1 || len(p.AuthInfo.SignerInfos) != 1 || len(p.Signatures) != 1 {
		return nil, "", ex.S3Error("INVALID_ENVELOPE")
	}
	b, fee, si := p.Body, p.AuthInfo.Fee, p.AuthInfo.SignerInfos[0]
	if b.Memo != "" || len(b.ExtensionOptions) > 0 || len(b.NonCriticalExtensionOptions) > 0 || fee.Payer != "" || fee.Granter != "" || p.AuthInfo.Tip != nil || b.Unordered || b.TimeoutTimestamp != nil {
		return nil, "", ex.S3Error("UNSUPPORTED_OPTION")
	}
	if si == nil || si.PublicKey == nil || si.PublicKey.TypeUrl != "/cosmos.crypto.mldsa65.PubKey" || si.ModeInfo == nil || si.ModeInfo.GetSingle() == nil || si.ModeInfo.GetSingle().Mode != signing.SignMode_SIGN_MODE_DIRECT || len(p.Signatures[0]) != 3309 {
		return nil, "", ex.S3Error("INVALID_ENVELOPE")
	}
	if si.Sequence == ^uint64(0) {
		return nil, "", ex.S3Error("SEQUENCE_OVERFLOW")
	}
	maxBytes, gas, feeAtoms := 16384, uint64(500000), int64(1000)
	owner := ""
	large := false
	switch m := t.GetMsgs()[0].(type) {
	case *ext.MsgDeposit:
		owner = m.Owner
	case *ext.MsgWithdraw:
		owner = m.Owner
	case *s3.MsgSettleBatch:
		owner = m.Operator
		maxBytes = ex.MaxSettleTxBytes
		gas = 10000000
		feeAtoms = 20000
		large = true
	case *s3.MsgCloseBatch:
		owner = m.Operator
		maxBytes = ex.MaxSettleTxBytes
		gas = 3000000
		feeAtoms = 6000
		large = true
	case *s3.MsgRevokeOrder:
		owner = m.Owner
	case *s3.MsgBumpOrderEpoch:
		owner = m.Owner
	case *s3.MsgRotateSettlementOperator:
		owner = m.Authority
	default:
		return nil, "", ex.S3Error("INVALID_ENVELOPE")
	}
	if rawSize > maxBytes {
		return nil, "", ex.S3Error("RESOURCE_LIMIT")
	}
	if large != (b.TimeoutHeight > 0) {
		return nil, "", ex.S3Error("UNSUPPORTED_OPTION")
	}
	if fee.GasLimit != gas || len(fee.Amount) != 1 || fee.Amount[0].Denom != ex.Gas || !fee.Amount[0].Amount.Equal(sdkmath.NewInt(feeAtoms)) {
		return nil, "", ex.S3Error("INVALID_ENVELOPE")
	}
	return p, owner, nil
}
func (a *App) guardS3(ctx sdk.Context, t sdk.Tx) error {
	p, owner, e := envelopeS3(t, len(ctx.TxBytes()))
	if e != nil {
		return e
	}
	var pub mldsa65.PubKey
	if e = a.Codec.Unmarshal(p.AuthInfo.SignerInfos[0].PublicKey.Value, &pub); e != nil || len(pub.Key) != 1952 {
		return ex.S3Error("KEY_LENGTH")
	}
	addr, e := sdk.AccAddressFromBech32(owner)
	if e != nil || addr.String() != owner || !bytes.Equal(addr, pub.Address()) {
		return ex.S3Error("ADDRESS_MISMATCH")
	}
	ac := a.Auth.GetAccount(ctx, addr)
	if ac == nil || ac.GetPubKey() == nil || !ac.GetPubKey().Equals(&pub) {
		return ex.S3Error("ACCOUNT_KEY_MISMATCH")
	}
	switch t.GetMsgs()[0].(type) {
	case *s3.MsgSettleBatch, *s3.MsgCloseBatch:
		if a.Exchange.Config(ctx).Operator != owner {
			return ex.S3Error("OPERATOR_UNAUTHORIZED")
		}
	case *s3.MsgRotateSettlementOperator:
		if a.Exchange.Config(ctx).Admin != owner {
			return ex.S3Error("OPERATOR_UNAUTHORIZED")
		}
	default:
		if !bytes.Equal(a.Exchange.S3Get(ctx, "user/"+owner), pub.Key) {
			return ex.S3Error("ACCOUNT_KEY_UNREGISTERED")
		}
	}
	return nil
}

func (a *App) signatureCost(raw []byte) (int, uint64, error) {
	t, e := a.TxConfig.TxDecoder()(raw)
	if e != nil {
		return 0, 0, e
	}
	p, _, e := envelopeS3(t, len(raw))
	if e != nil {
		return 0, 0, e
	}
	checks := 1
	switch m := t.GetMsgs()[0].(type) {
	case *s3.MsgSettleBatch:
		b, e := ex.DecodeBatch(m.BatchWire)
		if e != nil {
			return 0, 0, e
		}
		checks += len(b.Orders)
	case *s3.MsgCloseBatch:
		if _, e := ex.DecodeBatch(m.BatchWire); e != nil {
			return 0, 0, e
		}
	}
	return checks, p.AuthInfo.Fee.GasLimit, nil
}
func (a *App) blockBudget(txs [][]byte) error {
	checks, size := 0, 0
	gas := uint64(0)
	for _, raw := range txs {
		if len(raw) > ex.MaxSettleTxBytes {
			return ex.S3Error("RESOURCE_LIMIT")
		}
		n, g, e := a.signatureCost(raw)
		if e != nil {
			return e
		}
		checks += n
		size += len(raw)
		gas += g
		if checks > 34 || size > 1048576 || gas > 20000000 {
			return ex.S3Error("RESOURCE_LIMIT")
		}
	}
	return nil
}
func (a *App) configureS3() {
	a.SetPrepareProposal(func(_ sdk.Context, req *abci.RequestPrepareProposal) (*abci.ResponsePrepareProposal, error) {
		selected := [][]byte{}
		checks, size := 0, int64(0)
		gas := uint64(0)
		for _, raw := range req.Txs {
			if len(raw) > ex.MaxSettleTxBytes {
				continue
			}
			n, g, e := a.signatureCost(raw)
			if e != nil || checks+n > 34 || size+int64(len(raw)) > req.MaxTxBytes || size+int64(len(raw)) > 1048576 || gas+g > 20000000 {
				continue
			}
			selected = append(selected, raw)
			checks += n
			size += int64(len(raw))
			gas += g
		}
		return &abci.ResponsePrepareProposal{Txs: selected}, nil
	})
	a.SetProcessProposal(func(_ sdk.Context, req *abci.RequestProcessProposal) (*abci.ResponseProcessProposal, error) {
		status := abci.ResponseProcessProposal_ACCEPT
		if a.blockBudget(req.Txs) != nil {
			status = abci.ResponseProcessProposal_REJECT
		}
		return &abci.ResponseProcessProposal{Status: status}, nil
	})
	a.SetEndBlocker(func(ctx sdk.Context) (sdk.EndBlock, error) {
		for _, d := range []string{ex.Base, ex.Quote} {
			if e := a.Exchange.ReconcileAsset(ctx, d, true); e != nil {
				ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_s3_asset_deficit", sdk.NewAttribute("denom", d)))
			}
		}
		if ctx.BlockHeight() < 1 || len(ctx.HeaderHash()) != 32 || ctx.BlockTime().UnixMilli() < 0 {
			return sdk.EndBlock{}, fmt.Errorf("INVALID_BLOCK_CONTEXT")
		}
		a.Exchange.S3Set(ctx, "height", []byte(strconv.FormatInt(ctx.BlockHeight(), 10)))
		a.Exchange.S3Set(ctx, "block_hash", ctx.HeaderHash())
		a.Exchange.S3Set(ctx, "block_time", []byte(strconv.FormatInt(ctx.BlockTime().UnixMilli(), 10)))
		return sdk.EndBlock{}, nil
	})
}
func (a *App) FinalizeBlock(req *abci.RequestFinalizeBlock) (*abci.ResponseFinalizeBlock, error) {
	if a.Exchange.ChainID() == ex.S3ChainID {
		if e := a.blockBudget(req.Txs); e != nil {
			return nil, e
		}
	}
	return a.BaseApp.FinalizeBlock(req)
}
