//go:build dev_local_demo

package localkeys

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"sort"
	"strings"
	"testing"
	"time"

	app "github.com/nus-gang/cosmo-dex/chain/app"
)

func guardFixture(t *testing.T, fee string) app.LocalDemoInputs {
	return guardFixtureArtifacts(t, fee, "")
}
func guardFixtureArtifacts(t *testing.T, fee, artifacts string) app.LocalDemoInputs {
	t.Helper()
	const prefix = "proposals/s3-local-dev-v1/"
	const public = "proposals/s3-local-account-receipt-v1/"
	get := func(path string) []byte {
		raw, err := exec.Command("git", "show", "ed4cf278cff78312ac606d6834901e0b8b265725:"+path).Output()
		if err != nil {
			t.Fatal(err)
		}
		return raw
	}
	in := app.LocalDemoInputs{Files: map[string][]byte{}, AcknowledgeUnprovenSpace: true}
	for _, path := range []string{"protocol/s3/manifest.json", prefix + "MANIFEST.json"} {
		raw := get(path)
		in.Files[path] = raw
		var m struct {
			Files map[string]string `json:"files_sha256"`
		}
		if json.Unmarshal(raw, &m) != nil {
			t.Fatal("manifest")
		}
		for name := range m.Files {
			if strings.HasPrefix(path, prefix) {
				name = prefix + name
			}
			in.Files[name] = get(name)
		}
	}
	publicManifest := get(public + "MANIFEST.json")
	var publicFiles struct {
		Files map[string]string `json:"files_sha256"`
	}
	if json.Unmarshal(publicManifest, &publicFiles) != nil {
		t.Fatal("public manifest")
	}
	for name := range publicFiles.Files {
		if name != public+"MANIFEST.json" {
			in.Files[name] = get(name)
		}
	}
	in.Files[public+"MANIFEST.json"] = publicManifest
	components := map[string]string{}
	for _, name := range []string{"chain", "exchange", "settlement", "wallet", "sre"} {
		path := "chain/local-demo/components/" + name + ".json"
		components[name] = path
		in.Files[path] = []byte(`{"head":"` + strings.Repeat("1", 40) + `","tree":"` + strings.Repeat("2", 40) + `","implementation_settings":{"scope":"SYNTHETIC_OFFLINE_TEST_ONLY"}}`)
		if artifacts != "" {
			in.Files[path], _ = json.Marshal(map[string]any{"head": strings.Repeat("1", 40), "tree": strings.Repeat("2", 40), "implementation_settings": map[string]string{"scope": "SYNTHETIC_OFFLINE_TEST_ONLY", "artifacts_sha256_json": artifacts}})
		}
	}
	hashes := map[string]string{}
	names := []string{}
	for name, raw := range in.Files {
		hashes[name] = digest(raw)
		names = append(names, name)
	}
	sort.Strings(names)
	aggregate := ""
	for _, name := range names {
		aggregate += hashes[name] + "  " + name + "\n"
	}
	contract := digest([]byte(aggregate))
	in.RuntimeManifest, _ = json.Marshal(map[string]any{"format": "s3-dev-local-runtime/1", "scope": "REVIEWED_RUNTIME", "candidate_manifest_sha256": digest(in.Files[prefix+"MANIFEST.json"]), "public_receipt_manifest_sha256": digest(publicManifest), "public_receipt_schema_sha256": digest(in.Files[public+"schema.json"]), "public_receipt_version": "s3-dev-local-account/1", "contract_sha256": contract, "files_sha256": hashes, "components": components})
	// Synthetic pin exercises B byte checks only; never a runtime approval.
	in.ApprovedRuntimeSHA256 = digest(in.RuntimeManifest)
	in.EffectiveProfile = in.Files[prefix+"effective-profile-fee"+fee+".json"]
	keys := registrationKeys(t)
	nodes, err := GenerateFour()
	if err != nil {
		t.Fatal(err)
	}
	state, err := AppStateBytes(keys[:2], keys[2:4], keys[4], fee, contract, digest(in.EffectiveProfile))
	if err != nil {
		t.Fatal(err)
	}
	in.Genesis, err = GenesisBytes(nodes, time.Unix(1700000000, 0).UTC(), state)
	if err != nil {
		t.Fatal(err)
	}
	return in
}
func TestPrepareGuardBValidation(t *testing.T) {
	for _, fee := range []string{"0", "25"} {
		in := guardFixture(t, fee)
		raw, context, err := PrepareGuard(in, "11111111-2222-3333-4444-555555555555", "fee"+fee)
		if err != nil || len(context) != 7 || context["genesis_hash"] != digest(in.Genesis) || len(in.Guard) != 0 {
			t.Fatal("guard", err)
		}
		in.Guard = raw
		got, err := app.ValidateLocalDemo(in)
		if err != nil || fmt.Sprint(got) != fmt.Sprint(context) {
			t.Fatal("B validation", err)
		}
		in.Genesis = append(in.Genesis, ' ')
		if _, err := app.ValidateLocalDemo(in); err == nil {
			t.Fatal("changed genesis accepted")
		}
	}
}
func TestPrepareGuardRejects(t *testing.T) {
	in := guardFixture(t, "25")
	for _, kind := range []string{"uuid", "fee", "optin", "pin", "profile", "manifest", "genesis", "guard", "file"} {
		t.Run(kind, func(t *testing.T) {
			c := in
			uuid := "11111111-2222-3333-4444-555555555555"
			fee := "fee25"
			switch kind {
			case "uuid":
				uuid = "bad"
			case "fee":
				fee = "fee0"
			case "optin":
				c.AcknowledgeUnprovenSpace = false
			case "pin":
				c.ApprovedRuntimeSHA256 = strings.Repeat("0", 64)
			case "profile":
				c.EffectiveProfile = []byte("{}")
			case "manifest":
				c.RuntimeManifest = []byte("{}")
			case "genesis":
				c.Genesis = []byte("{}")
			case "guard":
				c.Guard = []byte("{}")
			case "file":
				c.Files = map[string][]byte{}
			}
			raw, ctx, err := PrepareGuard(c, uuid, fee)
			if err == nil || raw != nil || ctx != nil {
				t.Fatal("partial/accepted output")
			}
		})
	}
}
