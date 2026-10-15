//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"encoding/json"
	"testing"
	"time"

	cmttypes "github.com/cometbft/cometbft/types"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
)

func registrationKeys(t *testing.T) [][]byte {
	t.Helper()
	keys := [][]byte{}
	for i := 0; i < 5; i++ {
		key, err := mldsa65.GenPrivKeyFromSeed(bytes.Repeat([]byte{byte(i + 1)}, 32))
		if err != nil {
			t.Fatal(err)
		}
		keys = append(keys, key.PubKey().Bytes())
	}
	return keys
}
func TestPublicAppStateBDecoder(t *testing.T) {
	keys := registrationKeys(t)
	nodes, err := GenerateFour()
	if err != nil {
		t.Fatal(err)
	}
	for _, fee := range []string{"0", "25"} {
		config := ex.S3ConfigHash
		if fee == "25" {
			config = ex.S3Fee25ConfigHash
		}
		raw, err := AppStateBytes(keys[:2], keys[2:4], keys[4], fee, ex.S3ContractHash, config)
		if err != nil {
			t.Fatal(err)
		}
		// Standard hashes ONLY in this offline decoder test. No runtime approval.
		g, err := app.DecodeS3Genesis(raw)
		if err != nil || g.FeeBPS != fee || !bytes.Equal(g.AdminKey, keys[4]) {
			t.Fatal("B decode mismatch")
		}
		again, err := AppStateBytes(keys[:2], keys[2:4], keys[4], fee, ex.S3ContractHash, config)
		if err != nil || !bytes.Equal(raw, again) {
			t.Fatal("nondeterministic encoding")
		}
		genesis, err := GenesisBytes(nodes, time.Unix(1700000000, 0).UTC(), raw)
		if err != nil {
			t.Fatal(err)
		}
		doc, err := cmttypes.GenesisDocFromJSON(genesis)
		if err != nil || !bytes.Equal(doc.AppState, raw) {
			t.Fatal("app-state changed")
		}
		var fields map[string]json.RawMessage
		if json.Unmarshal(raw, &fields) != nil || len(fields) != 6 {
			t.Fatal("schema changed")
		}
		original := append([]byte(nil), raw...)
		keys[0][0] ^= 1
		if !bytes.Equal(raw, original) {
			t.Fatal("input alias")
		}
		keys[0][0] ^= 1
	}
}
func TestPublicAppStateRejects(t *testing.T) {
	keys := registrationKeys(t)
	cases := []struct {
		users, operators      [][]byte
		admin                 []byte
		fee, contract, config string
	}{
		{keys[:1], keys[2:4], keys[4], "0", ex.S3ContractHash, ex.S3ConfigHash},
		{keys[:2], keys[2:3], keys[4], "0", ex.S3ContractHash, ex.S3ConfigHash},
		{keys[:2], keys[2:4], keys[0], "0", ex.S3ContractHash, ex.S3ConfigHash},
		{keys[:2], keys[2:4], keys[4][:1951], "0", ex.S3ContractHash, ex.S3ConfigHash},
		{keys[:2], keys[2:4], keys[4], "00", ex.S3ContractHash, ex.S3ConfigHash},
		{keys[:2], keys[2:4], keys[4], "0", "bad", ex.S3ConfigHash},
		{keys[:2], keys[2:4], keys[4], "0", ex.S3ContractHash, "ABCDEF"},
	}
	for i, c := range cases {
		raw, err := AppStateBytes(c.users, c.operators, c.admin, c.fee, c.contract, c.config)
		if err == nil || raw != nil {
			t.Fatalf("case %d accepted", i)
		}
	}
	// Correctly sized but unapproved hashes remain untrusted; B rejects them.
	raw, err := AppStateBytes(keys[:2], keys[2:4], keys[4], "0", string(bytes.Repeat([]byte("0"), 64)), ex.S3ConfigHash)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = app.DecodeS3Genesis(raw); err == nil {
		t.Fatal("unapproved hash accepted by B")
	}
}
