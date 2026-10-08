//go:build dev_local_demo

package localkeys

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	app "github.com/nus-gang/cosmo-dex/chain/app"
)

func initializationFixture(t *testing.T, fee string) (*Initialization, string) {
	t.Helper()
	base := guardFixture(t, fee)
	base.Guard, base.Genesis = nil, nil
	keys := registrationKeys(t)
	prepared, err := PrepareInitialization(base, keys[:2], fee,
		"11111111-2222-4333-8444-555555555555", time.Unix(1800000000, 0).UTC())
	if err != nil {
		t.Fatal(err)
	}
	root := privateParent(t)
	return prepared, root
}

func validatorScript(t *testing.T, root, output string) string {
	t.Helper()
	path := filepath.Join(root, "validator")
	raw := []byte("#!/bin/sh\nprintf '%s' '" + output + "'\n")
	if err := os.WriteFile(path, raw, 0500); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestInitializationCValidationThenNoReplacePublish(t *testing.T) {
	for _, fee := range []string{"0", "25"} {
		prepared, parent := initializationFixture(t, fee)
		scratch := filepath.Join(parent, "scratch")
		if err := os.Mkdir(scratch, 0700); err != nil {
			t.Fatal(err)
		}
		validator := validatorScript(t, parent, cValidated)
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
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
		if report.FeeBPS != fee || !report.CValidated || report.ServiceStarted || len(report.NodeIDs) != 4 || len(report.Homes) != 4 {
			t.Fatal("public report")
		}
		seen := map[string]bool{}
		for i, home := range report.Homes {
			if seen[report.NodeIDs[i]] {
				t.Fatal("duplicate node id")
			}
			seen[report.NodeIDs[i]] = true
			for _, path := range []string{home, filepath.Join(home, "config"), filepath.Join(home, "data")} {
				info, err := os.Stat(path)
				if err != nil || info.Mode().Perm() != 0700 {
					t.Fatal("home mode")
				}
			}
			for _, path := range []string{filepath.Join(home, "guard.dev.json"), filepath.Join(home, "config", "genesis.json"), filepath.Join(home, "config", "priv_validator_key.json"), filepath.Join(home, "config", "node_key.json"), filepath.Join(home, "data", "priv_validator_state.json")} {
				info, err := os.Stat(path)
				if err != nil || info.Mode().Perm() != 0600 {
					t.Fatal("file mode")
				}
			}
		}
		raw, err := os.ReadFile(filepath.Join(root, "initialization.json"))
		if err != nil {
			t.Fatal(err)
		}
		var stored PublicInitialization
		if json.Unmarshal(raw, &stored) != nil || stored.GenesisSHA256 != report.GenesisSHA256 {
			t.Fatal("stored report")
		}
		if _, err := prepared.Publish(filepath.Join(parent, "again")); err == nil {
			t.Fatal("reused material")
		}
	}
}

func TestInitializationRejectsBeforePublishAndBadValidator(t *testing.T) {
	prepared, parent := initializationFixture(t, "0")
	root := filepath.Join(parent, "fee0")
	if _, err := prepared.Publish(root); err == nil || fileExists(root) {
		t.Fatal("publish before C validation")
	}
	scratch := filepath.Join(parent, "scratch")
	if err := os.Mkdir(scratch, 0700); err != nil {
		t.Fatal(err)
	}
	validator := validatorScript(t, parent, "WRONG\n")
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := prepared.ValidateC(ctx, validator, scratch); err == nil || fileExists(root) {
		t.Fatal("bad C result accepted")
	}
	prepared.Destroy()
}

// NUS-73 must not bypass this failure in the launcher. The reviewed B adapter
// currently accepts the older six-field manifest only, while the public account
// receipt contract adds three mandatory fields. The B owner must integrate and
// re-review that exact contract before this expectation can become success.
func TestPublicReceiptRuntimeManifestCurrentlyBlockedByB(t *testing.T) {
	base := guardFixture(t, "0")
	var manifest map[string]any
	if json.Unmarshal(base.RuntimeManifest, &manifest) != nil {
		t.Fatal("fixture manifest")
	}
	manifest["public_receipt_manifest_sha256"] = "5e911b9a5fc750c702c9c8cde09dcfc2c56106a7e0757f1d7ad2e839019b50fa"
	manifest["public_receipt_schema_sha256"] = "2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b"
	manifest["public_receipt_version"] = "s3-dev-local-account/1"
	base.RuntimeManifest, _ = json.Marshal(manifest)
	base.ApprovedRuntimeSHA256 = digest(base.RuntimeManifest)
	validated, err := app.ValidateLocalDemo(base)
	if err == nil || validated != nil || err.Error() != `json: unknown field "public_receipt_manifest_sha256"` {
		t.Fatalf("B public receipt rejection changed: %v", err)
	}
}

func fileExists(path string) bool { _, err := os.Lstat(path); return err == nil }
