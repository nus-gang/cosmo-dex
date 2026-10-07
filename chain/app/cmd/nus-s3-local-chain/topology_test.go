//go:build dev_local_demo

package main

import (
	"bytes"
	"fmt"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	"github.com/nus-gang/cosmo-dex/chain/app/internal/localkeys"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func topologyFixture(t *testing.T) ([]options, app.LocalDemoInputs, []string) {
	t.Helper()
	keys, e := localkeys.GenerateFour()
	if e != nil {
		t.Fatal(e)
	}
	raw, e := localkeys.GenesisBytes(keys, time.Unix(1700000000, 0).UTC(), []byte(`{"synthetic":true}`))
	if e != nil {
		t.Fatal(e)
	}
	in := app.LocalDemoInputs{Genesis: raw, Guard: []byte("SYNTHETIC_NOT_APPROVED")}
	ids := make([]string, 4)
	opts := make([]options, 4)
	for i, k := range keys {
		_, id := k.Public()
		ids[i] = string(id)
		root := privateDir(t)
		opts[i] = options{home: root, rpc: fmt.Sprintf("127.0.0.1:%d", 28000+2*i), p2p: fmt.Sprintf("127.0.0.1:%d", 28001+2*i)}
		for _, dir := range []string{"config", "data"} {
			if e = os.Mkdir(filepath.Join(root, dir), 0700); e != nil {
				t.Fatal(e)
			}
		}
		files := k.PrivateFiles()
		files["genesis.json"] = raw
		for name, b := range files {
			dir := "config"
			if name == "priv_validator_state.json" {
				dir = "data"
			}
			if e = os.WriteFile(filepath.Join(root, dir, name), b, 0600); e != nil {
				t.Fatal(e)
			}
		}
		if e = os.WriteFile(filepath.Join(root, "guard.dev.json"), in.Guard, 0600); e != nil {
			t.Fatal(e)
		}
	}
	for i := range opts {
		var peers []string
		for j := range opts {
			if i != j {
				peers = append(peers, ids[j]+"@"+opts[j].p2p)
			}
		}
		opts[i].peers = strings.Join(peers, ",")
	}
	return opts, in, ids
}
func homeBytes(t *testing.T, opts []options) map[string]string {
	t.Helper()
	out := map[string]string{}
	for _, o := range opts {
		e := filepath.WalkDir(o.home, func(p string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if !d.IsDir() {
				b, e := os.ReadFile(p)
				if e != nil {
					return e
				}
				out[p] = string(b)
			}
			return nil
		})
		if e != nil {
			t.Fatal(e)
		}
	}
	return out
}
func TestHomeTopologyReadOnly(t *testing.T) {
	opts, in, want := topologyFixture(t)
	before := homeBytes(t, opts)
	got, e := inspectHomeTopology(opts, in)
	if e != nil || fmt.Sprint(got) != fmt.Sprint(want) {
		t.Fatal("public identity mismatch", e)
	}
	after := homeBytes(t, opts)
	if len(before) != len(after) {
		t.Fatal("created file")
	}
	for p, b := range before {
		if b != after[p] {
			t.Fatal("changed home")
		}
	}
}
func TestHomeTopologyIdentityMismatch(t *testing.T) {
	for _, which := range []string{"peer", "node", "validator", "genesis", "guard", "port", "home"} {
		t.Run(which, func(t *testing.T) {
			opts, in, _ := topologyFixture(t)
			switch which {
			case "peer":
				opts[0].peers = strings.Repeat("a", 40) + opts[0].peers[40:]
			case "port":
				opts[1].rpc = opts[0].rpc
			case "home":
				opts[1].home = opts[0].home
			default:
				name := map[string]string{"node": "config/node_key.json", "validator": "config/priv_validator_key.json", "genesis": "config/genesis.json", "guard": "guard.dev.json"}[which]
				b, e := os.ReadFile(filepath.Join(opts[0].home, name))
				if e != nil {
					t.Fatal(e)
				}
				if which == "genesis" || which == "guard" {
					b = append(b, ' ')
				}
				if e = os.WriteFile(filepath.Join(opts[1].home, name), b, 0600); e != nil {
					t.Fatal(e)
				}
			}
			before := homeBytes(t, opts)
			got, e := inspectHomeTopology(opts, in)
			if e == nil || got != nil {
				t.Fatal("accepted mismatch")
			}
			after := homeBytes(t, opts)
			if len(before) != len(after) {
				t.Fatal("created file")
			}
			for p, b := range before {
				if !bytes.Equal([]byte(b), []byte(after[p])) {
					t.Fatal("changed home")
				}
			}
		})
	}
}
func TestHomeTopologyPrivatePathRefusal(t *testing.T) {
	for _, which := range []string{"mode", "symlink", "hardlink", "duplicate-json"} {
		t.Run(which, func(t *testing.T) {
			opts, in, _ := topologyFixture(t)
			p := filepath.Join(opts[0].home, "config/node_key.json")
			switch which {
			case "mode":
				if e := os.Chmod(p, 0644); e != nil {
					t.Fatal(e)
				}
			case "symlink":
				if e := os.Rename(p, p+".saved"); e != nil {
					t.Fatal(e)
				}
				if e := os.Symlink(p+".saved", p); e != nil {
					t.Fatal(e)
				}
			case "hardlink":
				if e := os.Link(p, p+".link"); e != nil {
					t.Fatal(e)
				}
			case "duplicate-json":
				if e := os.WriteFile(p, []byte(`{"priv_key":null,"priv_key":null}`), 0600); e != nil {
					t.Fatal(e)
				}
			}
			if ids, e := inspectHomeTopology(opts, in); e == nil || ids != nil {
				t.Fatal("unsafe home accepted")
			}
		})
	}
}
