//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"encoding/json"
	"fmt"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	"testing"
)

func TestAuthoritiesFreshRegistrationAndSigner(t *testing.T) {
	seen := map[string]bool{}
	users := registrationKeys(t)[:2]
	for _, fee := range []string{"0", "25"} {
		a, err := GenerateAuthorities()
		if err != nil {
			t.Fatal(err)
		}
		defer a.Destroy()
		ops, admin, err := a.Public()
		if err != nil {
			t.Fatal(err)
		}
		pubs := append(ops, admin)
		for role, pub := range pubs {
			if seen[string(pub)] {
				t.Fatal("key reuse")
			}
			seen[string(pub)] = true
			seed, err := a.Seed(role)
			if err != nil || len(seed) != 32 {
				t.Fatal("seed")
			}
			key, err := mldsa65.GenPrivKeyFromSeed(seed)
			clear(seed)
			if err != nil || !bytes.Equal(key.PubKey().Bytes(), pub) {
				t.Fatal("public mismatch")
			}
			sig, err := key.Sign([]byte("offline authority test"))
			clear(key.Key)
			if err != nil || !(&mldsa65.PubKey{Key: pub}).VerifySignature([]byte("offline authority test"), sig) {
				t.Fatal("signature")
			}
		}
		config := ex.S3ConfigHash
		if fee == "25" {
			config = ex.S3Fee25ConfigHash
		}
		raw, err := AppStateBytes(users, ops, admin, fee, ex.S3ContractHash, config)
		if err != nil {
			t.Fatal(err)
		}
		g, err := app.DecodeS3Genesis(raw)
		if err != nil || !bytes.Equal(g.AdminKey, admin) || !bytes.Equal(g.OperatorKeys[1], ops[1]) {
			t.Fatal("registration")
		}
	}
}
func TestAuthoritiesCopiesDestroyAndRedaction(t *testing.T) {
	a, err := GenerateAuthorities()
	if err != nil {
		t.Fatal(err)
	}
	alias := *a
	seed, _ := a.Seed(0)
	original := append([]byte(nil), seed...)
	seed[0] ^= 1
	again, _ := a.Seed(0)
	if !bytes.Equal(again, original) {
		t.Fatal("seed alias")
	}
	clear(seed)
	clear(original)
	clear(again)
	ops, admin, _ := a.Public()
	ops[0][0] ^= 1
	admin[0] ^= 1
	next, nextAdmin, _ := a.Public()
	if bytes.Equal(next[0], ops[0]) || bytes.Equal(nextAdmin, admin) {
		t.Fatal("public alias")
	}
	for _, value := range []any{a, *a} {
		if _, err := json.Marshal(value); err == nil {
			t.Fatal("serialized secret")
		}
		if fmt.Sprintf("%v", value) != "localkeys.Authorities(REDACTED)" || fmt.Sprintf("%#v", value) != "localkeys.Authorities(REDACTED)" {
			t.Fatal("logging")
		}
	}
	if _, err := a.Seed(-1); err == nil {
		t.Fatal("role")
	}
	if _, err := a.Seed(3); err == nil {
		t.Fatal("role")
	}
	a.Destroy()
	a.Destroy()
	for _, v := range []*Authorities{a, &alias, nil, {}} {
		if raw, err := v.Seed(0); err == nil || raw != nil {
			t.Fatal("closed seed")
		}
		if ops, admin, err := v.Public(); err == nil || ops != nil || admin != nil {
			t.Fatal("closed public")
		}
	}
	for _, seed := range a.state.seeds {
		if seed != [32]byte{} {
			t.Fatal("owned seed not cleared")
		}
	}
}
func TestAuthoritiesEntropyFailureAndDuplicate(t *testing.T) {
	for _, n := range []int{0, 31, 32, 63, 64, 95, 96} {
		// A repeated deterministic seed is a failure fixture, never a runtime input.
		if a, err := generateAuthorities(bytes.NewReader(make([]byte, n))); err == nil || a != nil {
			t.Fatal("failure returned material")
		}
	}
}
