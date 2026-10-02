package app

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"cosmossdk.io/log/v2"
	abci "github.com/cometbft/cometbft/abci/types"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	sdk "github.com/cosmos/cosmos-sdk/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
)

func s2Fixture(t *testing.T) *fixture {
	old := newFixture(t)
	f := &fixture{db: dbm.NewMemDB(), hash: bytes.Repeat([]byte{7}, 32), keys: old.keys}
	var err error
	f.a, err = NewForChain(f.db, f.hash, log.NewNopLogger(), ex.S2ChainID)
	mustTest(t, err)
	raw, err := json.Marshal(Genesis{PublicKeys: [][]byte{f.keys[0].PubKey().Bytes(), f.keys[1].PubKey().Bytes()}, OperatorAccounts: testOperators(t)})
	mustTest(t, err)
	params := cmttypes.DefaultConsensusParams().ToProto()
	_, err = f.a.InitChain(&abci.RequestInitChain{ChainId: ex.S2ChainID, AppStateBytes: raw, ConsensusParams: &params})
	mustTest(t, err)
	f.block(t, nil)
	return f
}
func s2Message(f *fixture, user int, id byte, withdraw bool, denom, amount, epoch string) sdk.Msg {
	m := f.message(user, id, withdraw, amount, epoch)
	switch v := m.(type) {
	case *ext.MsgDeposit:
		v.Denom = denom
	case *ext.MsgWithdraw:
		v.Denom = denom
	}
	return m
}
func s2OK(t *testing.T, f *fixture, user int, m sdk.Msg) {
	t.Helper()
	r := f.block(t, f.sign(t, user, m, ex.S2ChainID))
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	mustTest(t, f.a.Exchange.Invariant(f.ctx(t)))
}
func TestS2TwoAssetSignedLedger(t *testing.T) {
	f := s2Fixture(t)
	// Actual SDK DIRECT signatures -> FinalizeBlock -> Commit; no C injection.
	s2OK(t, f, 0, s2Message(f, 0, 1, false, ex.Base, "10000000", "0"))
	s2OK(t, f, 1, s2Message(f, 1, 1, false, ex.Quote, "100000000", "0"))
	s2OK(t, f, 0, s2Message(f, 0, 2, false, ex.Quote, "20000000", "0"))
	h := f.h
	before, err := f.a.snapshot(f.ctx(t))
	mustTest(t, err)
	withdrawal := s2Message(f, 0, 3, true, ex.Base, "3000000", "0")
	raw := f.sign(t, 0, withdrawal, ex.S2ChainID)
	r := f.block(t, raw)
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	for _, denom := range []string{ex.Base, ex.Quote} {
		p := f.a.Exchange.AssetPosition(f.ctx(t), f.owner(0), denom)
		if p.Epoch != "1" {
			t.Fatal("epoch not shared", p)
		}
	}
	if f.a.Exchange.AssetPosition(f.ctx(t), f.owner(0), ex.Base).Amount != "7000000" {
		t.Fatal("base debit")
	}
	if f.a.Exchange.Position(f.ctx(t), f.owner(0)).Amount != "20000000" {
		t.Fatal("quote changed")
	}
	if r = f.block(t, raw); r.Code == 0 {
		t.Fatal("raw replay accepted")
	}
	// A re-signed identical request preserves original receipt, with no second debit/epoch.
	receipt, _ := f.a.Exchange.Receipt(f.ctx(t), f.owner(0), strings.Repeat("03", 32))
	s2OK(t, f, 0, withdrawal)
	afterReceipt, _ := f.a.Exchange.Receipt(f.ctx(t), f.owner(0), strings.Repeat("03", 32))
	if receipt != afterReceipt || receipt.Denom != ex.Base || receipt.ChainID != ex.S2ChainID {
		t.Fatal("retry receipt")
	}
	for _, denom := range []string{ex.Base, ex.Quote} {
		m := s2Message(f, 0, 4, true, denom, "1", "0")
		r = f.block(t, f.sign(t, 0, m, ex.S2ChainID))
		if r.Code == 0 || !strings.Contains(r.Log, "EPOCH_MISMATCH") {
			t.Fatal("old shared epoch accepted", r)
		}
	}
	conflict := s2Message(f, 0, 3, true, ex.Quote, "3000000", "0")
	r = f.block(t, f.sign(t, 0, conflict, ex.S2ChainID))
	if r.Code == 0 || !strings.Contains(r.Log, "ID_CONFLICT") {
		t.Fatal("cross-asset ID conflict accepted")
	}
	// Historical query is one committed state, including its persisted block identity.
	ctx, err := f.a.CreateQueryContext(h, false)
	mustTest(t, err)
	historic, err := f.a.snapshot(ctx)
	mustTest(t, err)
	if !reflect.DeepEqual(before, historic) {
		t.Fatal("historic snapshot mixed heights")
	}
	body := historic["body"].(map[string]any)
	if body["block_hash"] != hex.EncodeToString(bytes.Repeat([]byte{byte(h)}, 32)) {
		t.Fatal("wrong H block hash")
	}
	latest, err := f.a.snapshot(f.ctx(t))
	mustTest(t, err)
	a, err := NewForChain(f.db, f.hash, log.NewNopLogger(), ex.S2ChainID)
	mustTest(t, err)
	f.a = a
	// SDK initializes the latest query header on the first post-restart block.
	f.block(t, nil)
	restoredCtx, err := f.a.CreateQueryContext(f.h-1, false)
	mustTest(t, err)
	reloaded, err := f.a.snapshot(restoredCtx)
	mustTest(t, err)
	if !reflect.DeepEqual(latest, reloaded) {
		t.Fatal("reload changed snapshot")
	}
	if _, err = New(f.db, f.hash, log.NewNopLogger()); err == nil {
		t.Fatal("S2 DB loaded as S1")
	}
	if dir := os.Getenv("S2_EVIDENCE_DIR"); dir != "" {
		mustTest(t, os.MkdirAll(dir, 0700))
		raw, err := json.MarshalIndent(latest, "", "  ")
		mustTest(t, err)
		mustTest(t, os.WriteFile(filepath.Join(dir, "chain-snapshot.json"), raw, 0600))
	}
}
func TestS2RejectedTransactions(t *testing.T) {
	for _, tc := range []struct {
		name, denom, epoch, chain string
		tamper                    bool
	}{
		{"gas", ex.Gas, "0", ex.S2ChainID, false},
		{"unknown", "OTHER", "0", ex.S2ChainID, false},
		{"old_epoch", ex.Base, "1", ex.S2ChainID, false},
		{"s1_domain", ex.Base, "0", ex.ChainID, false},
		{"signature", ex.Base, "0", ex.S2ChainID, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			f := s2Fixture(t)
			m := s2Message(f, 0, 1, false, tc.denom, "10", tc.epoch)
			raw := f.sign(t, 0, m, tc.chain)
			if tc.tamper {
				raw[len(raw)-1] ^= 1
			}
			r := f.block(t, raw)
			if r.Code == 0 {
				t.Fatal("invalid accepted")
			}
			for _, denom := range []string{ex.Base, ex.Quote} {
				p := f.a.Exchange.AssetPosition(f.ctx(t), f.owner(0), denom)
				if p.Amount != "0" || p.Epoch != "0" {
					t.Fatal("partial effect")
				}
			}
			if _, ok := f.a.Exchange.Receipt(f.ctx(t), f.owner(0), strings.Repeat("01", 32)); ok {
				t.Fatal("failed receipt persisted")
			}
			mustTest(t, f.a.Exchange.Invariant(f.ctx(t)))
		})
	}
}
func TestS1RejectsS2Asset(t *testing.T) {
	f := newFixture(t)
	m := s2Message(f, 0, 1, false, ex.Base, "1", "0")
	if r := f.block(t, f.sign(t, 0, m, ex.ChainID)); r.Code == 0 {
		t.Fatal("S1 accepted DEVBASE")
	}
}

func TestS2WithdrawBlockOrderAndRollback(t *testing.T) {
	f := s2Fixture(t)
	for i, denom := range []string{ex.Base, ex.Quote} {
		s2OK(t, f, 0, s2Message(f, 0, byte(i+1), false, denom, "10", "0"))
	}
	// The first debit invalidates the other asset at the shared owner epoch.
	first := f.sign(t, 0, s2Message(f, 0, 3, true, ex.Base, "1", "0"), ex.S2ChainID)
	// Sign second after the first committed transaction, then both epoch cases
	// are checked explicitly. Signature sequence and owner epoch are independent.
	if r := f.block(t, first); r.Code != 0 {
		t.Fatal(r.Log)
	}
	for _, denom := range []string{ex.Base, ex.Quote} {
		r := f.block(t, f.sign(t, 0, s2Message(f, 0, 4, true, denom, "1", "0"), ex.S2ChainID))
		if r.Code == 0 || !strings.Contains(r.Log, "EPOCH_MISMATCH") {
			t.Fatal(r.Log)
		}
	}
	before := f.a.Exchange.AssetPosition(f.ctx(t), f.owner(0), ex.Base)
	r := f.block(t, f.sign(t, 0, s2Message(f, 0, 5, true, ex.Base, "10", "1"), ex.S2ChainID))
	if r.Code == 0 || !strings.Contains(r.Log, "INSUFFICIENT_CONFIRMED_BALANCE") {
		t.Fatal(r.Log)
	}
	if f.a.Exchange.AssetPosition(f.ctx(t), f.owner(0), ex.Base) != before {
		t.Fatal("overdraft changed state")
	}
	ctx, _ := f.ctx(t).CacheContext()
	ctx.KVStore(f.a.Exchange.Key).Set([]byte("e/"+f.owner(0)), []byte("18446744073709551615"))
	m := s2Message(f, 0, 6, true, ex.Base, "1", "18446744073709551615").(*ext.MsgWithdraw)
	_, err := f.a.Exchange.Withdraw(ctx, m)
	if err == nil || !strings.Contains(err.Error(), "INTEGER_RANGE") {
		t.Fatal(err)
	}
	if f.a.Exchange.AssetPosition(ctx, f.owner(0), ex.Base).Amount != before.Amount {
		t.Fatal("overflow debit")
	}
	mustTest(t, f.a.Exchange.Invariant(f.ctx(t)))
}
