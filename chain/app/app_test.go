package app

import (
	"bytes"
	"context"
	"encoding/json"
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
	g, e := json.Marshal(Genesis{[][]byte{f.keys[0].PubKey().Bytes(), f.keys[1].PubKey().Bytes()}})
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
	module, _ := sdkmath.NewIntFromString(s["module_atoms"].(string))
	collector, _ := sdkmath.NewIntFromString(s["gas_collector_atoms"].(string))
	if !c.Equal(module) || b.Add(module).String() != "2000000000000" || g.Add(collector).String() != "2000000000" {
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
