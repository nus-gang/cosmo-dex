package app

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"cosmossdk.io/log/v2"
	sdkmath "cosmossdk.io/math"
	abci "github.com/cometbft/cometbft/abci/types"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	clienttx "github.com/cosmos/cosmos-sdk/client/tx"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authsigning "github.com/cosmos/cosmos-sdk/x/auth/signing"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	banktypes "github.com/cosmos/cosmos-sdk/x/bank/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
)

func mustTest(t *testing.T, e error) {
	t.Helper()
	if e != nil {
		t.Fatal(e)
	}
}

type fixture struct {
	db   dbm.DB
	a    *App
	keys []mldsa65.PrivKey
	h    int64
	hash []byte
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	f := &fixture{hash: bytes.Repeat([]byte{3}, 32)}
	for i := 0; i < 2; i++ {
		seed := make([]byte, 32)
		seed[0] = byte(i + 1)
		k, e := mldsa65.GenPrivKeyFromSeed(seed)
		mustTest(t, e)
		f.keys = append(f.keys, k)
	}
	f.db = dbm.NewMemDB()
	a, e := New(f.db, f.hash, log.NewNopLogger())
	mustTest(t, e)
	f.a = a
	g, e := json.Marshal(Genesis{PublicKeys: [][]byte{f.keys[0].PubKey().Bytes(), f.keys[1].PubKey().Bytes()}, OperatorAccounts: testOperators(t)})
	mustTest(t, e)
	params := cmttypes.DefaultConsensusParams().ToProto()
	_, e = a.InitChain(&abci.RequestInitChain{ChainId: ex.ChainID, AppStateBytes: g, ConsensusParams: &params})
	mustTest(t, e)
	f.block(t, nil)
	return f
}
func (f *fixture) block(t *testing.T, raw []byte) *abci.ExecTxResult {
	t.Helper()
	f.h++
	var txs [][]byte
	if raw != nil {
		txs = [][]byte{raw}
	}
	r, e := f.a.FinalizeBlock(&abci.RequestFinalizeBlock{Height: f.h, Time: time.Unix(f.h, 0), Txs: txs, Hash: bytes.Repeat([]byte{byte(f.h)}, 32)})
	mustTest(t, e)
	_, e = f.a.Commit()
	mustTest(t, e)
	if raw != nil {
		return r.TxResults[0]
	}
	return nil
}
func (f *fixture) ctx(t *testing.T) sdk.Context {
	c, e := f.a.CreateQueryContext(0, false)
	mustTest(t, e)
	return c
}
func (f *fixture) owner(i int) string { return sdk.AccAddress(f.keys[i].PubKey().Address()).String() }
func (f *fixture) message(i int, id byte, withdraw bool, amount, epoch string) sdk.Msg {
	rid := bytes.Repeat([]byte{id}, 32)
	if withdraw {
		return &ext.MsgWithdraw{Owner: f.owner(i), Denom: ex.Quote, AmountAtoms: amount, RequestId: rid, ExpectedEpoch: epoch, ExpiryHeight: "1000", GenesisHash: f.hash}
	}
	return &ext.MsgDeposit{Owner: f.owner(i), Denom: ex.Quote, AmountAtoms: amount, RequestId: rid, ExpectedEpoch: epoch, ExpiryHeight: "1000", GenesisHash: f.hash}
}
func (f *fixture) sign(t *testing.T, i int, msg sdk.Msg, chain string) []byte {
	t.Helper()
	k := f.keys[i]
	ac := f.a.Auth.GetAccount(f.ctx(t), sdk.AccAddress(k.PubKey().Address()))
	b := f.a.TxConfig.NewTxBuilder()
	mustTest(t, b.SetMsgs(msg))
	b.SetGasLimit(500000)
	b.SetFeeAmount(sdk.NewCoins(sdk.NewCoin(ex.Gas, sdkmath.NewInt(1000))))
	seq := ac.GetSequence()
	mustTest(t, b.SetSignatures(signing.SignatureV2{PubKey: k.PubKey(), Data: &signing.SingleSignatureData{SignMode: signing.SignMode_SIGN_MODE_DIRECT}, Sequence: seq}))
	sig, e := clienttx.SignWithPrivKey(context.Background(), signing.SignMode_SIGN_MODE_DIRECT, authsigning.SignerData{Address: ac.GetAddress().String(), ChainID: chain, AccountNumber: ac.GetAccountNumber(), Sequence: seq, PubKey: k.PubKey()}, b, &k, f.a.TxConfig, seq)
	mustTest(t, e)
	mustTest(t, b.SetSignatures(sig))
	raw, e := f.a.TxConfig.TxEncoder()(b.GetTx())
	mustTest(t, e)
	return raw
}
func (f *fixture) conserved(t *testing.T) {
	t.Helper()
	s, e := f.a.snapshot(f.ctx(t))
	mustTest(t, e)
	users := s["accounts"].([]map[string]any)
	b, c, g := sdkmath.ZeroInt(), sdkmath.ZeroInt(), sdkmath.ZeroInt()
	for _, u := range users {
		v, _ := sdkmath.NewIntFromString(u["bank_atoms"].(string))
		b = b.Add(v)
		v, _ = sdkmath.NewIntFromString(u["exchange_atoms"].(string))
		c = c.Add(v)
		v, _ = sdkmath.NewIntFromString(u["gas_atoms"].(string))
		g = g.Add(v)
	}
	for _, op := range s["operator_accounts"].([]map[string]any) {
		v, _ := sdkmath.NewIntFromString(op["gas_atoms"].(string))
		g = g.Add(v)
	}
	module, _ := sdkmath.NewIntFromString(s["module_atoms"].(string))
	collector, _ := sdkmath.NewIntFromString(s["gas_collector_atoms"].(string))
	if !c.Equal(module) || b.Add(module).String() != "2000000000000" || g.Add(collector).String() != "6000000000" {
		t.Fatalf("conservation failed %v", s)
	}
}
func TestTwoUsersAtomicRetryAndSequence(t *testing.T) {
	f := newFixture(t)
	for i := 0; i < 2; i++ {
		m := f.message(i, 1, false, "1000000", "0")
		raw := f.sign(t, i, m, ex.ChainID)
		r := f.block(t, raw)
		if r.Code != 0 {
			t.Fatal(r.Log)
		}
		f.conserved(t)
		r = f.block(t, raw)
		if r.Code == 0 {
			t.Fatal("same raw replay accepted")
		}
		f.conserved(t)
		w := f.message(i, 2, true, "500000", "0")
		r = f.block(t, f.sign(t, i, w, ex.ChainID))
		if r.Code != 0 {
			t.Fatal(r.Log)
		}
		p := f.a.Exchange.Position(f.ctx(t), f.owner(i))
		if p.Amount != "500000" || p.Epoch != "1" {
			t.Fatal(p)
		}
		// Exact message re-signed at a new sequence retains original receipt and epoch.
		before, _ := f.a.Exchange.Receipt(f.ctx(t), f.owner(i), strings.Repeat("02", 32))
		r = f.block(t, f.sign(t, i, w, ex.ChainID))
		if r.Code != 0 {
			t.Fatal(r.Log)
		}
		after, _ := f.a.Exchange.Receipt(f.ctx(t), f.owner(i), strings.Repeat("02", 32))
		if before != after {
			t.Fatal("receipt changed")
		}
		p = f.a.Exchange.Position(f.ctx(t), f.owner(i))
		if p.Amount != "500000" || p.Epoch != "1" {
			t.Fatal(p)
		}
		f.conserved(t)
		conflict := f.message(i, 2, false, "1", "1")
		r = f.block(t, f.sign(t, i, conflict, ex.ChainID))
		if r.Code == 0 || !strings.Contains(r.Log, "ID_CONFLICT") {
			t.Fatal(r)
		}
		f.conserved(t)
	}
}
func TestRejectedTransactionsRollback(t *testing.T) {
	cases := []struct {
		name   string
		edit   func(*fixture, *ext.MsgDeposit)
		chain  string
		tamper bool
	}{
		{name: "zero", edit: func(f *fixture, m *ext.MsgDeposit) { m.AmountAtoms = "0" }},
		{name: "leading_zero", edit: func(f *fixture, m *ext.MsgDeposit) { m.AmountAtoms = "01" }},
		{name: "negative", edit: func(f *fixture, m *ext.MsgDeposit) { m.AmountAtoms = "-1" }},
		{name: "limit", edit: func(f *fixture, m *ext.MsgDeposit) { m.AmountAtoms = "1000000000001" }},
		{name: "u128_overflow", edit: func(f *fixture, m *ext.MsgDeposit) { m.AmountAtoms = "340282366920938463463374607431768211456" }},
		{name: "epoch", edit: func(f *fixture, m *ext.MsgDeposit) { m.ExpectedEpoch = "1" }},
		{name: "epoch_overflow", edit: func(f *fixture, m *ext.MsgDeposit) { m.ExpectedEpoch = "18446744073709551616" }},
		{name: "expiry_equal", edit: func(f *fixture, m *ext.MsgDeposit) { m.ExpiryHeight = "2" }},
		{name: "genesis", edit: func(f *fixture, m *ext.MsgDeposit) { m.GenesisHash = bytes.Repeat([]byte{9}, 32) }},
		{name: "owner", edit: func(f *fixture, m *ext.MsgDeposit) { m.Owner = f.owner(1) }},
		{name: "chain", chain: "wrong-chain"}, {name: "signature", tamper: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			f := newFixture(t)
			m := f.message(0, 4, false, "1", "0").(*ext.MsgDeposit)
			if tc.edit != nil {
				tc.edit(f, m)
			}
			chain := tc.chain
			if chain == "" {
				chain = ex.ChainID
			}
			raw := f.sign(t, 0, m, chain)
			if tc.tamper {
				raw[len(raw)-1] ^= 1
			}
			r := f.block(t, raw)
			if r.Code == 0 {
				t.Fatal("accepted invalid transaction")
			}
			p := f.a.Exchange.Position(f.ctx(t), f.owner(0))
			if p.Amount != "0" || p.Epoch != "0" {
				t.Fatal(p)
			}
			if _, ok := f.a.Exchange.Receipt(f.ctx(t), f.owner(0), strings.Repeat("04", 32)); ok {
				t.Fatal("failed receipt persisted")
			}
			f.conserved(t)
		})
	}
}
func TestWithdrawAndBankBypass(t *testing.T) {
	f := newFixture(t)
	for _, m := range []sdk.Msg{f.message(0, 1, true, "1", "0"), &banktypes.MsgSend{FromAddress: f.owner(0), ToAddress: f.owner(1), Amount: sdk.NewCoins(sdk.NewCoin(ex.Quote, sdkmath.NewInt(1)))}} {
		r := f.block(t, f.sign(t, 0, m, ex.ChainID))
		if r.Code == 0 {
			t.Fatal("bypass or overdraft")
		}
		f.conserved(t)
	}
}

func TestMaximumAmountAndInsufficientBank(t *testing.T) {
	f := newFixture(t)
	r := f.block(t, f.sign(t, 0, f.message(0, 8, false, "1000000000000", "0"), ex.ChainID))
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	r = f.block(t, f.sign(t, 0, f.message(0, 9, false, "1", "0"), ex.ChainID))
	if r.Code == 0 || !strings.Contains(r.Log, "INSUFFICIENT_BANK_BALANCE") {
		t.Fatal(r)
	}
	f.conserved(t)
}
func TestExpiredRetryPreservesReceipt(t *testing.T) {
	f := newFixture(t)
	m := f.message(0, 7, false, "10", "0").(*ext.MsgDeposit)
	m.ExpiryHeight = "3"
	r := f.block(t, f.sign(t, 0, m, ex.ChainID))
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	r = f.block(t, f.sign(t, 0, m, ex.ChainID))
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	p := f.a.Exchange.Position(f.ctx(t), f.owner(0))
	if p.Amount != "10" {
		t.Fatal(p)
	}
	f.conserved(t)
}
func TestFailedMessageConsumesOnlyGasAndSequence(t *testing.T) {
	f := newFixture(t)
	r := f.block(t, f.sign(t, 0, f.message(0, 1, true, "1", "0"), ex.ChainID))
	if r.Code == 0 {
		t.Fatal("overdraft accepted")
	}
	ctx := f.ctx(t)
	addr := sdk.AccAddress(f.keys[0].PubKey().Address())
	if f.a.Auth.GetAccount(ctx, addr).GetSequence() != 1 || f.a.Bank.GetBalance(ctx, addr, ex.Gas).Amount.String() != "999999000" {
		t.Fatal("ante effects missing")
	}
	if f.a.Bank.GetBalance(ctx, addr, ex.Quote).Amount.String() != "1000000000000" {
		t.Fatal("quote moved on failure")
	}
}
func TestKeeperEpochOverflowRollsBack(t *testing.T) {
	f := newFixture(t)
	r := f.block(t, f.sign(t, 0, f.message(0, 1, false, "10", "0"), ex.ChainID))
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	ctx, _ := f.ctx(t).CacheContext()
	ctx.KVStore(f.a.Exchange.Key).Set([]byte("p/"+f.owner(0)), []byte(`{"exchange_atoms":"10","epoch":"18446744073709551615"}`))
	m := f.message(0, 2, true, "1", "18446744073709551615").(*ext.MsgWithdraw)
	_, e := f.a.Exchange.Withdraw(ctx, m)
	if e == nil || !strings.Contains(e.Error(), "INTEGER_RANGE") {
		t.Fatal(e)
	}
	if f.a.Exchange.Position(ctx, f.owner(0)).Amount != "10" {
		t.Fatal("partial debit")
	}
}

func TestReloadGenesisBinding(t *testing.T) {
	f := newFixture(t)
	r := f.block(t, f.sign(t, 0, f.message(0, 1, false, "10", "0"), ex.ChainID))
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	a, e := New(f.db, f.hash, log.NewNopLogger())
	mustTest(t, e)
	if a.LastBlockHeight() != f.h {
		t.Fatal("lost height")
	}
	if _, e = New(f.db, bytes.Repeat([]byte{8}, 32), log.NewNopLogger()); e == nil {
		t.Fatal("changed genesis accepted")
	}
}

func testOperators(t *testing.T) []OperatorAccount {
	t.Helper()
	out := []OperatorAccount{}
	for i := 0; i < 4; i++ {
		seed := make([]byte, 32)
		seed[0] = byte(i + 3)
		key, err := mldsa65.GenPrivKeyFromSeed(seed)
		mustTest(t, err)
		out = append(out, OperatorAccount{sdk.AccAddress(key.PubKey().Address()).String(), "1000000000"})
	}
	return out
}

func TestOperatorGenesisValidation(t *testing.T) {
	f := newFixture(t)
	makeGenesis := func() Genesis {
		return Genesis{PublicKeys: [][]byte{f.keys[0].PubKey().Bytes(), f.keys[1].PubKey().Bytes()}, OperatorAccounts: testOperators(t)}
	}
	cases := []struct {
		name string
		edit func(*Genesis)
	}{
		{"missing", func(g *Genesis) { g.OperatorAccounts = nil }},
		{"three", func(g *Genesis) { g.OperatorAccounts = g.OperatorAccounts[:3] }},
		{"five", func(g *Genesis) { g.OperatorAccounts = append(g.OperatorAccounts, g.OperatorAccounts[0]) }},
		{"duplicate", func(g *Genesis) { g.OperatorAccounts[1] = g.OperatorAccounts[0] }},
		{"user_overlap", func(g *Genesis) { g.OperatorAccounts[0].Address = f.owner(0) }},
		{"module", func(g *Genesis) { g.OperatorAccounts[0].Address = authtypes.NewModuleAddress(ex.Module).String() }},
		{"collector", func(g *Genesis) {
			g.OperatorAccounts[0].Address = authtypes.NewModuleAddress(authtypes.FeeCollectorName).String()
		}},
		{"authority", func(g *Genesis) {
			g.OperatorAccounts[0].Address = authtypes.NewModuleAddress("disabled-authority").String()
		}},
		{"bad_address", func(g *Genesis) { g.OperatorAccounts[0].Address = "nus1bad" }},
		{"uppercase_address", func(g *Genesis) { g.OperatorAccounts[0].Address = strings.ToUpper(g.OperatorAccounts[0].Address) }},
		{"short_address", func(g *Genesis) { g.OperatorAccounts[0].Address = sdk.AccAddress([]byte{1}).String() }},
		{"zero", func(g *Genesis) { g.OperatorAccounts[0].GasAtoms = "0" }},
		{"negative", func(g *Genesis) { g.OperatorAccounts[0].GasAtoms = "-1" }},
		{"leading_zero", func(g *Genesis) { g.OperatorAccounts[0].GasAtoms = "01" }},
		{"fraction", func(g *Genesis) { g.OperatorAccounts[0].GasAtoms = "1.0" }},
		{"empty", func(g *Genesis) { g.OperatorAccounts[0].GasAtoms = "" }},
		{"u64_overflow", func(g *Genesis) { g.OperatorAccounts[0].GasAtoms = "18446744073709551616" }},
		{"total_overflow", func(g *Genesis) { g.OperatorAccounts[0].GasAtoms = "18446744073709551615" }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			g := makeGenesis()
			tc.edit(&g)
			raw, err := json.Marshal(g)
			mustTest(t, err)
			if _, err = DecodeGenesis(raw); err == nil {
				t.Fatal("invalid allocation accepted")
			}
		})
	}
	t.Run("unknown_quote_allocation", func(t *testing.T) {
		raw, err := json.Marshal(makeGenesis())
		mustTest(t, err)
		raw = bytes.Replace(raw, []byte(`"gas_atoms":"1000000000"`), []byte(`"gas_atoms":"1000000000","quote_atoms":"1"`), 1)
		if _, err = DecodeGenesis(raw); err == nil {
			t.Fatal("unknown allocation accepted")
		}
	})
	for _, field := range []string{`"gas_atoms":"1000000000"`, `"gas_\u0061toms":"1000000000"`} {
		t.Run("duplicate_field_"+field, func(t *testing.T) {
			raw, err := json.Marshal(makeGenesis())
			mustTest(t, err)
			raw = bytes.Replace(raw, []byte(`"gas_atoms":"1000000000"`), []byte(`"gas_atoms":"1000000000",`+field), 1)
			if _, err = DecodeGenesis(raw); err == nil {
				t.Fatal("duplicate field accepted")
			}
		})
	}
	t.Run("numeric_atoms", func(t *testing.T) {
		raw, err := json.Marshal(makeGenesis())
		mustTest(t, err)
		raw = bytes.Replace(raw, []byte(`"gas_atoms":"1000000000"`), []byte(`"gas_atoms":1000000000`), 1)
		if _, err = DecodeGenesis(raw); err == nil {
			t.Fatal("number accepted")
		}
	})
	t.Run("u64_total_boundary", func(t *testing.T) {
		g := makeGenesis()
		g.OperatorAccounts[0].GasAtoms = "18446744071709551612"
		for i := 1; i < 4; i++ {
			g.OperatorAccounts[i].GasAtoms = "1"
		}
		mustTest(t, g.Validate())
		g.OperatorAccounts[3].GasAtoms = "2"
		if g.Validate() == nil {
			t.Fatal("sum overflow accepted")
		}
	})
}

func TestGenesisExactFieldNames(t *testing.T) {
	f := newFixture(t)
	raw, err := json.Marshal(Genesis{PublicKeys: [][]byte{f.keys[0].PubKey().Bytes(), f.keys[1].PubKey().Bytes()}, OperatorAccounts: testOperators(t)})
	mustTest(t, err)
	_, err = DecodeGenesis(raw)
	mustTest(t, err)
	for _, field := range []string{"public_keys", "operator_accounts", "address", "gas_atoms"} {
		for _, alias := range []string{strings.ToUpper(field), strings.ToUpper(field[:1]) + field[1:]} {
			for _, mode := range []string{"alias_only", "alias_first", "alias_last"} {
				t.Run(field+"/"+alias+"/"+mode, func(t *testing.T) {
					replacement := `"` + alias + `":`
					if mode == "alias_first" {
						replacement += `null,"` + field + `":`
					}
					if mode == "alias_last" {
						replacement = `"` + field + `":null,` + replacement
					}
					candidate := bytes.Replace(raw, []byte(`"`+field+`":`), []byte(replacement), 1)
					if _, err := DecodeGenesis(candidate); err == nil || !strings.Contains(err.Error(), "UNKNOWN_JSON_FIELD") {
						t.Fatalf("case alias not rejected by exact-name check: %v", err)
					}
				})
			}
		}
		t.Run(field+"/escaped_duplicate", func(t *testing.T) {
			escaped := `\u` + fmt.Sprintf("%04x", field[0]) + field[1:]
			candidate := bytes.Replace(raw, []byte(`"`+field+`":`), []byte(`"`+escaped+`":null,"`+field+`":`), 1)
			if _, err := DecodeGenesis(candidate); err == nil {
				t.Fatal("escaped duplicate accepted")
			}
		})
	}
}

func TestOperatorsCannotTransactAndPersist(t *testing.T) {
	f := newFixture(t)
	for i := 0; i < 4; i++ {
		seed := make([]byte, 32)
		seed[0] = byte(i + 3)
		key, err := mldsa65.GenPrivKeyFromSeed(seed)
		mustTest(t, err)
		f.keys = append(f.keys, key)
	}
	for i := 2; i < 6; i++ {
		addr := sdk.AccAddress(f.keys[i].PubKey().Address())
		ac := f.a.Auth.GetAccount(f.ctx(t), addr)
		if ac == nil || ac.GetPubKey() != nil || ac.GetAccountNumber() != uint64(i) {
			t.Fatalf("bad operator account: %v", ac)
		}
		for _, withdraw := range []bool{false, true} {
			msg := f.message(i, byte(20+i), withdraw, "1", "0")
			r := f.block(t, f.sign(t, i, msg, ex.ChainID))
			if r.Code == 0 || !strings.Contains(r.Log, "UNAUTHORIZED") {
				t.Fatal(r)
			}
			if f.a.Auth.GetAccount(f.ctx(t), addr).GetSequence() != 0 ||
				f.a.Bank.GetBalance(f.ctx(t), addr, ex.Gas).Amount.String() != "1000000000" {
				t.Fatal("operator ante effects persisted")
			}
		}
	}
	for i := 0; i < 2; i++ {
		for n, withdraw := range []bool{false, true} {
			r := f.block(t, f.sign(t, i, f.message(i, byte(n+1), withdraw, "10", "0"), ex.ChainID))
			if r.Code != 0 {
				t.Fatal(r)
			}
		}
	}
	f.conserved(t)
	before, err := f.a.snapshot(f.ctx(t))
	mustTest(t, err)
	a, err := New(f.db, f.hash, log.NewNopLogger())
	mustTest(t, err)
	f.a = a
	f.block(t, nil)
	after, err := f.a.snapshot(f.ctx(t))
	mustTest(t, err)
	delete(before, "observed_height")
	delete(after, "observed_height")
	if !reflect.DeepEqual(before, after) {
		t.Fatal("restart changed ledger")
	}
	f.conserved(t)
	for _, op := range after["operator_accounts"].([]map[string]any) {
		if op["bank_atoms"] != "0" || op["gas_atoms"] != "1000000000" || op["exchange_signer"] != false {
			t.Fatal(op)
		}
	}
	ctx, _ := f.ctx(t).CacheContext()
	ctx.KVStore(a.Exchange.Key).Set([]byte("genesis_gas_supply"), []byte("1"))
	if err = a.Exchange.Invariant(ctx); err == nil || !strings.Contains(err.Error(), "GAS_SUPPLY_INVARIANT") {
		t.Fatal("gas genesis binding missing", err)
	}
}
