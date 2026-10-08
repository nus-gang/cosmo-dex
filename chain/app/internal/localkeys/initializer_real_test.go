//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	app "github.com/nus-gang/cosmo-dex/chain/app"
)

func TestInitializerActualCAndPublishedBundle(t *testing.T) {
	validator := os.Getenv("NUS_C_VALIDATOR")
	if validator == "" {
		t.Skip("explicit offline C validator required")
	}
	for _, fee := range []string{"0", "25"} {
		prepared, parent := initializationFixture(t, fee)
		defer prepared.Destroy()
		original := append([]byte{}, prepared.bundle...)
		profile := append([]byte{}, prepared.in.EffectiveProfile...)
		scratch := filepath.Join(parent, "scratch")
		if err := os.Mkdir(scratch, 0700); err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		if err := prepared.ValidateC(ctx, validator, scratch); err != nil {
			cancel()
			t.Fatal(err)
		}
		cancel()
		root := filepath.Join(parent, "fee"+fee)
		report, err := prepared.Publish(root)
		if err != nil {
			t.Fatal(err)
		}
		raw, err := os.ReadFile(filepath.Join(root, "input.json"))
		if err != nil || !bytes.Equal(raw, original) {
			t.Fatal("published transport changed")
		}
		got, err := os.ReadFile(filepath.Join(root, "effective-profile.json"))
		if err != nil || !bytes.Equal(got, profile) {
			t.Fatal("published profile changed")
		}
		var decoded struct {
			Manifest []byte            `json:"runtime_manifest"`
			Files    map[string][]byte `json:"files"`
			Guard    []byte            `json:"guard"`
			Genesis  []byte            `json:"genesis"`
		}
		if json.Unmarshal(raw, &decoded) != nil {
			t.Fatal("decode")
		}
		in := app.LocalDemoInputs{ApprovedRuntimeSHA256: report.RuntimePin, RuntimeManifest: decoded.Manifest,
			Files: decoded.Files, Guard: decoded.Guard, Genesis: decoded.Genesis, EffectiveProfile: got, AcknowledgeUnprovenSpace: true}
		if _, err := app.ValidateLocalDemo(in); err != nil {
			t.Fatal(err)
		}
		for _, home := range report.Homes {
			genesis, _ := os.ReadFile(filepath.Join(home, "config", "genesis.json"))
			guard, _ := os.ReadFile(filepath.Join(home, "guard.dev.json"))
			if !bytes.Equal(genesis, decoded.Genesis) || !bytes.Equal(guard, decoded.Guard) {
				t.Fatal("home input mismatch")
			}
		}
		if entries, _ := os.ReadDir(scratch); len(entries) != 0 {
			t.Fatal("scratch not cleared")
		}
		// A separately generated identity set may not replace this output.
		second, _ := initializationFixture(t, fee)
		defer second.Destroy()
		second.cValidated = true // publication-only collision seam; no service use
		if _, err := second.Publish(root); err == nil {
			t.Fatal("replaced home")
		}
		after, _ := os.ReadFile(filepath.Join(root, "input.json"))
		if !bytes.Equal(after, original) {
			t.Fatal("collision modified evidence")
		}
	}
}
