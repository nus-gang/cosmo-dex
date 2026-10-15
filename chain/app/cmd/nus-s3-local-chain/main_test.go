//go:build dev_local_demo

package main

import (
	cmtcfg "github.com/cometbft/cometbft/config"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
	cmttypes "github.com/cometbft/cometbft/types"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestOptInAndArgumentRefusal(t *testing.T) {
	base := []string{"preflight", "--local-demo-profile", "/profile", "--acknowledge-unproven-space", "--input-set", "/bundle", "--runtime-pin", strings.Repeat("a", 64)}
	if _, e := parse(base); e != nil {
		t.Fatal(e)
	}
	cases := [][]string{nil, {"version"}, {"preflight"}, {"preflight", "--local-demo-profile", "/p"}, {"preflight", "--acknowledge-unproven-space"}, append(append([]string{}, base...), "--home", "/a", "--home", "/b"), append(append([]string{}, base...), "--unknown", "x"), append(append([]string{}, base...), "--acknowledge-unproven-space"), append(append([]string{}, base...), "--home=/x"), append(append([]string{}, base...), "stray")}
	for i, args := range cases {
		if _, e := parse(args); e == nil {
			t.Fatalf("case %d accepted", i)
		}
	}
}
func TestStrictJSON(t *testing.T) {
	for _, raw := range []string{`{"a":1,"a":2}`, `{"files":{"x":"YQ==","x":"Yg=="}}`, `{} {}`, `{"a":[{"b":1,"b":2}]}`, `{"a":`} {
		if uniqueJSON([]byte(raw)) == nil {
			t.Fatalf("accepted %s", raw)
		}
	}
	if e := uniqueJSON([]byte(`{"a":[1,true,null,{"b":"x"}]}`)); e != nil {
		t.Fatal(e)
	}
}
func privateDir(t *testing.T) string {
	t.Helper()
	p, e := filepath.EvalSymlinks(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	if e = os.Chmod(p, 0700); e != nil {
		t.Fatal(e)
	}
	return p
}
func TestReadFileMetadata(t *testing.T) {
	root := privateDir(t)
	p := filepath.Join(root, "input")
	if e := os.WriteFile(p, []byte("ok"), 0600); e != nil {
		t.Fatal(e)
	}
	if _, e := readFile(p, 2, true); e != nil {
		t.Fatal(e)
	}
	if _, e := readFile(p, 1, true); e == nil {
		t.Fatal("size")
	}
	if e := os.Symlink(p, filepath.Join(root, "sym")); e != nil {
		t.Fatal(e)
	}
	if _, e := readFile(filepath.Join(root, "sym"), 2, true); e == nil {
		t.Fatal("symlink")
	}
	if e := os.Link(p, filepath.Join(root, "hard")); e != nil {
		t.Fatal(e)
	}
	if _, e := readFile(p, 2, true); e == nil {
		t.Fatal("hardlink")
	}
	if e := os.Remove(filepath.Join(root, "hard")); e != nil {
		t.Fatal(e)
	}
	if e := os.Chmod(p, 0644); e != nil {
		t.Fatal(e)
	}
	if _, e := readFile(p, 2, true); e == nil {
		t.Fatal("secret mode")
	}
	if _, e := readFile("relative", 2, false); e == nil {
		t.Fatal("relative")
	}
	alias := filepath.Join(root, "alias")
	if e := os.Symlink(root, alias); e != nil {
		t.Fatal(e)
	}
	if _, e := readFile(filepath.Join(alias, "input"), 2, false); e == nil {
		t.Fatal("parent symlink")
	}
}
func TestLoopbackOnly(t *testing.T) {
	for _, a := range []string{"127.0.0.1:26657", "[::1]:26657"} {
		if e := loopback(a); e != nil {
			t.Fatal(a, e)
		}
	}
	for _, a := range []string{"localhost:26657", "0.0.0.0:26657", "192.168.1.1:26657", "127.0.0.1:0", "127.0.0.1:80", "127.0.0.1:026657", "tcp://127.0.0.1:26657", "127.0.0.1:65536", "[::]:26657"} {
		if loopback(a) == nil {
			t.Fatal(a)
		}
	}
}
func TestConfigExposureClosed(t *testing.T) {
	o := options{home: "/private/synthetic", rpc: "127.0.0.1:27657", p2p: "127.0.0.1:27656", peers: strings.Repeat("a", 40) + "@127.0.0.1:27666," + strings.Repeat("b", 40) + "@127.0.0.1:27676," + strings.Repeat("c", 40) + "@127.0.0.1:27686"}
	c, e := config(o)
	if e != nil {
		t.Fatal(e)
	}
	if c.P2P.PexReactor || c.RPC.Unsafe || c.RPC.GRPCListenAddress != "" || c.Instrumentation.Prometheus || len(c.RPC.CORSAllowedOrigins) != 0 {
		t.Fatal("exposure")
	}
	o.peers = strings.Replace(o.peers, "127.0.0.1:27686", "0.0.0.0:27686", 1)
	if _, e = config(o); e == nil {
		t.Fatal("public peer")
	}
}
func TestWriterExclusionAndRelease(t *testing.T) {
	root := privateDir(t)
	a, e := lock(root)
	if e != nil {
		t.Fatal(e)
	}
	if b, e := lock(root); e == nil {
		b.Close()
		t.Fatal("writer2 accepted")
	}
	if e = a.Close(); e != nil {
		t.Fatal(e)
	}
	b, e := lock(root)
	if e != nil {
		t.Fatal("lock not released", e)
	}
	b.Close()
	if _, e = os.Stat(filepath.Join(root, "writer.dev.lock")); e != nil {
		t.Fatal("lock inode must remain")
	}
}
func TestWriterRefusesLink(t *testing.T) {
	root := privateDir(t)
	p := filepath.Join(root, "target")
	os.WriteFile(p, []byte{}, 0600)
	l := filepath.Join(root, "writer.dev.lock")
	if e := os.Symlink(p, l); e != nil {
		t.Fatal(e)
	}
	if f, e := lock(root); e == nil {
		f.Close()
		t.Fatal("symlink")
	}
	os.Remove(l)
	if e := os.Link(p, l); e != nil {
		t.Fatal(e)
	}
	if f, e := lock(root); e == nil {
		f.Close()
		t.Fatal("hardlink")
	}
}
func TestRefusalHasNoSideEffects(t *testing.T) {
	root := privateDir(t)
	home := filepath.Join(root, "absent")
	e := run([]string{"start", "--home", home, "--local-demo-profile", "/missing"})
	if e == nil {
		t.Fatal("opt-in accepted")
	}
	if _, e = os.Stat(home); !os.IsNotExist(e) {
		t.Fatal("created home")
	}
}

func TestHomeBindingAndKeyPreflightWithoutService(t *testing.T) {
	root := privateDir(t)
	os.Mkdir(filepath.Join(root, "config"), 0700)
	os.Mkdir(filepath.Join(root, "data"), 0700)
	c := cmtcfg.DefaultConfig().SetRoot(root)
	pv := privval.GenFilePV(c.PrivValidatorKeyFile(), c.PrivValidatorStateFile())
	pv.Save()
	pub, e := pv.GetPubKey()
	if e != nil {
		t.Fatal(e)
	}
	_, e = p2p.LoadOrGenNodeKey(c.NodeKeyFile())
	if e != nil {
		t.Fatal(e)
	}
	g := cmttypes.GenesisDoc{ChainID: "synthetic-key-check-only", Validators: []cmttypes.GenesisValidator{{Address: pub.Address(), PubKey: pub, Power: 10}}}
	raw, e := cmtjson.Marshal(g)
	if e != nil {
		t.Fatal(e)
	}
	guard := []byte("SYNTHETIC_BINDING_TEST_NOT_APPROVED")
	os.WriteFile(c.GenesisFile(), raw, 0600)
	os.WriteFile(filepath.Join(root, "guard.dev.json"), guard, 0600)
	in := app.LocalDemoInputs{Genesis: raw, Guard: guard}
	o := options{home: root}
	if e = validateHome(o, in, c); e != nil {
		t.Fatal(e)
	}
	if _, e = os.Stat(filepath.Join(root, "writer.dev.lock")); !os.IsNotExist(e) {
		t.Fatal("preflight wrote lock")
	}
	if _, e = os.Stat(filepath.Join(root, "data/application.db")); !os.IsNotExist(e) {
		t.Fatal("preflight opened DB")
	}
	in.Guard = []byte("changed")
	if validateHome(o, in, c) == nil {
		t.Fatal("guard mismatch")
	}
	in.Guard = guard
	target := filepath.Join(root, "data", "linked")
	os.Symlink(c.GenesisFile(), target)
	if validateHome(o, in, c) == nil {
		t.Fatal("linked DB")
	}
	os.Remove(target)
	os.WriteFile(c.NodeKeyFile(), []byte(`{"priv_key":null}`), 0600)
	if validateHome(o, in, c) == nil {
		t.Fatal("missing node key")
	}
}
