//go:build dev_local_demo

package localkeys

import (
	"context"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestPublishedAuthoritiesRustSigner(t *testing.T) {
	bridge := os.Getenv("NUS_AUTHORITY_TEST_BRIDGE")
	if bridge == "" {
		t.Skip("explicit Rust test bridge required")
	}
	if !filepath.IsAbs(bridge) {
		t.Fatal("absolute bridge required")
	}
	parent := privateParent(t)
	document := []byte("NUS-73 fresh authority cross-language test")
	invoke := func(dir, pub string, accept bool) []byte {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		cmd := exec.CommandContext(ctx, bridge, dir, pub)
		cmd.Env = []string{"PATH=/usr/bin:/bin"}
		out, err := cmd.Output()
		if ctx.Err() != nil {
			t.Fatal("bridge timeout")
		}
		if accept && (err != nil || len(out) != 3309) {
			t.Fatal("bridge signature failed")
		}
		if !accept && (err == nil || len(out) != 0) {
			t.Fatal("bridge rejection failed")
		}
		return out
	}
	for _, fee := range []string{"fee0", "fee25"} {
		a, err := GenerateAuthorities()
		if err != nil {
			t.Fatal(err)
		}
		pubs, _, err := a.Public()
		if err != nil {
			t.Fatal(err)
		}
		root := filepath.Join(parent, fee)
		if err = a.Publish(root); err != nil {
			t.Fatal(err)
		}
		for i, role := range []string{"operator-0", "operator-1"} {
			dir := filepath.Join(root, role)
			pubfile := filepath.Join(root, role+".public")
			if err = os.WriteFile(pubfile, pubs[i], 0600); err != nil {
				t.Fatal(err)
			}
			sig := invoke(dir, pubfile, true)
			pk := &mldsa65.PubKey{Key: pubs[i]}
			if !pk.VerifySignature(document, sig) || pk.VerifySignature([]byte("changed"), sig) {
				t.Fatal("SDK verification")
			}
			if err = os.WriteFile(pubfile, pubs[1-i], 0600); err != nil {
				t.Fatal(err)
			}
			invoke(dir, pubfile, false)
			if err = os.WriteFile(pubfile, pubs[i], 0600); err != nil {
				t.Fatal(err)
			}
			seed := filepath.Join(dir, "operator.seed")
			if err = os.Chmod(seed, 0644); err != nil {
				t.Fatal(err)
			}
			invoke(dir, pubfile, false)
			if err = os.Chmod(seed, 0600); err != nil {
				t.Fatal(err)
			}
			invoke(dir, pubfile, true)
		}
	}
}
