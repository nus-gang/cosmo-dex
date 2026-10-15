//go:build dev_local_demo

package localkeys

import (
	"encoding/json"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	cmttypes "github.com/cometbft/cometbft/types"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestStagedChainRealPreflight(t *testing.T) {
	binary, python, bridge := os.Getenv("NUS_CHAIN_BINARY"), os.Getenv("NUS_PYTHON"), os.Getenv("NUS_CHAIN_BRIDGE")
	if binary == "" || python == "" || bridge == "" {
		t.Skip("explicit compiled chain and Python bridge required")
	}
	rawBinary, err := os.ReadFile(binary)
	if err != nil {
		t.Fatal(err)
	}
	inventory, _ := json.Marshal(map[string]string{"bin/nus-s3-local-chain": digest(rawBinary)})
	for _, fee := range []string{"0", "25"} {
		in := guardFixtureArtifacts(t, fee, string(inventory))
		var old cmttypes.GenesisDoc
		if err := cmtjson.Unmarshal(in.Genesis, &old); err != nil {
			t.Fatal(err)
		}
		nodes, err := GenerateFour()
		if err != nil {
			t.Fatal(err)
		}
		in.Genesis, err = GenesisBytes(nodes, time.Unix(1700000000, 0).UTC(), old.AppState)
		if err != nil {
			t.Fatal(err)
		}
		in.Guard, _, err = PrepareGuard(in, "11111111-2222-3333-4444-555555555555", "fee"+fee)
		if err != nil {
			t.Fatal(err)
		}
		input, err := InputBundle(in)
		if err != nil {
			t.Fatal(err)
		}
		root, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		write := func(name string, data []byte) {
			t.Helper()
			p := filepath.Join(root, name)
			if err := os.MkdirAll(filepath.Dir(p), 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(p, data, 0600); err != nil {
				t.Fatal(err)
			}
		}
		write("input.json", input)
		write("profile.json", in.EffectiveProfile)
		write("bundle/runtime-manifest.json", in.RuntimeManifest)
		for name, raw := range in.Files {
			write("bundle/files/"+name, raw)
		}
		write("artifacts/bin/nus-s3-local-chain", rawBinary)
		write("home/guard.dev.json", in.Guard)
		write("home/config/genesis.json", in.Genesis)
		for name, raw := range nodes[0].PrivateFiles() {
			dir := "config/"
			if name == "priv_validator_state.json" {
				dir = "data/"
			}
			write("home/"+dir+name, raw)
		}
		peers := ""
		for i := 1; i < 4; i++ {
			_, id := nodes[i].Public()
			if i > 1 {
				peers += ","
			}
			peers += string(id) + "@127.0.0.1:" + []string{"27666", "27676", "27686"}[i-1]
		}
		cmd := exec.Command(python, bridge, root, in.ApprovedRuntimeSHA256, peers)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("bridge fee%s: %v %s", fee, err, out)
		}
		if string(out) != "REAL_CHAIN_PREFLIGHT_PASS\n" {
			t.Fatal("bridge report")
		}
	}
}
