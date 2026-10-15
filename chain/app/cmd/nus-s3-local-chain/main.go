//go:build dev_local_demo

// nus-s3-local-chain wires the reviewed B constructor to a local Comet node.
// Runtime approval is external; preflight never creates a DB, home or listener.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"os/signal"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"syscall"
	"time"

	"cosmossdk.io/log/v2"
	cmtcfg "github.com/cometbft/cometbft/config"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	cmtlog "github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/node"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
	"github.com/cometbft/cometbft/proxy"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	"github.com/cosmos/cosmos-sdk/server"
	app "github.com/nus-gang/cosmo-dex/chain/app"
)

type options struct {
	mode, profile, bundle, pin, home, rpc, p2p, peers string
	ack                                               bool
}

func parse(args []string) (o options, err error) {
	if len(args) == 0 || (args[0] != "preflight" && args[0] != "start") {
		return o, errors.New("MODE_REQUIRED")
	}
	o.mode = args[0]
	f := flag.NewFlagSet(o.mode, flag.ContinueOnError)
	f.SetOutput(io.Discard)
	f.StringVar(&o.profile, "local-demo-profile", "", "")
	f.BoolVar(&o.ack, "acknowledge-unproven-space", false, "")
	f.StringVar(&o.bundle, "input-set", "", "")
	f.StringVar(&o.pin, "runtime-pin", "", "")
	f.StringVar(&o.home, "home", "", "")
	f.StringVar(&o.rpc, "rpc", "", "")
	f.StringVar(&o.p2p, "p2p", "", "")
	f.StringVar(&o.peers, "peers", "", "")
	// flag normally accepts duplicates and aliases; keep one exact spelling.
	seen := map[string]bool{}
	for i := 1; i < len(args); i++ {
		k := args[i]
		if !strings.HasPrefix(k, "--") || strings.Contains(k, "=") || seen[k] {
			return o, errors.New("INVALID_OR_DUPLICATE_OPTION")
		}
		seen[k] = true
		if k != "--acknowledge-unproven-space" {
			i++
			if i >= len(args) || strings.HasPrefix(args[i], "--") {
				return o, errors.New("OPTION_VALUE_REQUIRED")
			}
		}
	}
	if err = f.Parse(args[1:]); err != nil {
		return o, errors.New("INVALID_OPTION")
	}
	if !o.ack || o.profile == "" {
		return o, errors.New("LOCAL_DEMO_OPT_IN_REQUIRED")
	}
	if o.bundle == "" || !regexp.MustCompile(`^[0-9a-f]{64}$`).MatchString(o.pin) {
		return o, errors.New("INPUT_AND_INDEPENDENT_PIN_REQUIRED")
	}
	return o, nil
}

// Check every path segment before open, then compare the opened inode. Inputs
// live in a private operator-owned workspace; no untrusted process may mutate it.
func safePath(path string) error {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return errors.New("CANONICAL_ABSOLUTE_PATH_REQUIRED")
	}
	cur := string(filepath.Separator)
	for _, part := range strings.Split(strings.TrimPrefix(path, cur), cur) {
		cur = filepath.Join(cur, part)
		s, e := os.Lstat(cur)
		if e != nil {
			return e
		}
		if s.Mode()&os.ModeSymlink != 0 {
			return errors.New("SYMLINK_REJECTED")
		}
	}
	return nil
}
func readFile(path string, max int64, private bool) ([]byte, error) {
	if e := safePath(path); e != nil {
		return nil, e
	}
	before, e := os.Lstat(path)
	if e != nil {
		return nil, e
	}
	st, ok := before.Sys().(*syscall.Stat_t)
	if !ok || !before.Mode().IsRegular() || st.Nlink != 1 || before.Size() > max || (private && before.Mode().Perm() != 0600) {
		return nil, errors.New("FILE_METADATA_REJECTED")
	}
	fd, e := syscall.Open(path, syscall.O_RDONLY|syscall.O_NOFOLLOW, 0)
	if e != nil {
		return nil, e
	}
	f := os.NewFile(uintptr(fd), path)
	defer f.Close()
	after, e := f.Stat()
	if e != nil || !os.SameFile(before, after) {
		return nil, errors.New("FILE_CHANGED")
	}
	raw, e := io.ReadAll(io.LimitReader(f, max+1))
	if e != nil || int64(len(raw)) > max {
		return nil, errors.New("INPUT_SIZE")
	}
	return raw, nil
}

// Reject duplicate keys recursively, including file-map keys, before Go's
// struct decoder can silently choose the final occurrence.
func uniqueJSON(raw []byte) error {
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	var value func() error
	value = func() error {
		t, e := d.Token()
		if e != nil {
			return e
		}
		switch t {
		case json.Delim('{'):
			seen := map[string]bool{}
			for d.More() {
				k, e := d.Token()
				if e != nil {
					return e
				}
				s, ok := k.(string)
				if !ok || seen[s] {
					return errors.New("DUPLICATE_JSON_KEY")
				}
				seen[s] = true
				if e = value(); e != nil {
					return e
				}
			}
			t, e = d.Token()
			if e != nil || t != json.Delim('}') {
				return errors.New("JSON_OBJECT")
			}
		case json.Delim('['):
			for d.More() {
				if e = value(); e != nil {
					return e
				}
			}
			t, e = d.Token()
			if e != nil || t != json.Delim(']') {
				return errors.New("JSON_ARRAY")
			}
		}
		return nil
	}
	if e := value(); e != nil {
		return e
	}
	if _, e := d.Token(); e != io.EOF {
		return errors.New("JSON_TRAILING")
	}
	return nil
}
func inputs(o options) (app.LocalDemoInputs, error) {
	in := app.LocalDemoInputs{ApprovedRuntimeSHA256: o.pin, AcknowledgeUnprovenSpace: o.ack}
	raw, e := readFile(o.bundle, 48<<20, false)
	if e != nil {
		return in, e
	}
	if e = uniqueJSON(raw); e != nil {
		return in, e
	}
	var fields map[string]json.RawMessage
	if e = json.Unmarshal(raw, &fields); e != nil {
		return in, e
	}
	if len(fields) != 4 {
		return in, errors.New("BUNDLE_FIELDS")
	}
	for _, key := range []string{"runtime_manifest", "files", "guard", "genesis"} {
		if fields[key] == nil || bytes.Equal(fields[key], []byte("null")) {
			return in, errors.New("BUNDLE_FIELDS")
		}
	}
	var b struct {
		Manifest []byte            `json:"runtime_manifest"`
		Files    map[string][]byte `json:"files"`
		Guard    []byte            `json:"guard"`
		Genesis  []byte            `json:"genesis"`
	}
	if e = json.Unmarshal(raw, &b); e != nil {
		return in, e
	}
	in.RuntimeManifest = b.Manifest
	in.Files = b.Files
	in.Guard = b.Guard
	in.Genesis = b.Genesis
	in.EffectiveProfile, e = readFile(o.profile, 1<<20, false)
	if e != nil {
		return in, e
	}
	_, e = app.ValidateLocalDemo(in)
	return in, e
}
func loopback(address string) error {
	h, p, e := net.SplitHostPort(address)
	if e != nil {
		return errors.New("LOOPBACK_ADDRESS_REQUIRED")
	}
	ip := net.ParseIP(h)
	port, e := strconv.Atoi(p)
	if e != nil || port < 1024 || port > 65535 || strconv.Itoa(port) != p || ip == nil || !ip.IsLoopback() || ip.String() != h {
		return errors.New("LOOPBACK_ADDRESS_REQUIRED")
	}
	return nil
}
func config(o options) (*cmtcfg.Config, error) {
	if e := loopback(o.rpc); e != nil {
		return nil, e
	}
	if e := loopback(o.p2p); e != nil {
		return nil, e
	}
	if o.rpc == o.p2p {
		return nil, errors.New("PORT_COLLISION")
	}
	peers := strings.Split(o.peers, ",")
	if len(peers) != 3 {
		return nil, errors.New("THREE_PEERS_REQUIRED")
	}
	seen := map[string]bool{}
	seenAddr := map[string]bool{o.rpc: true, o.p2p: true}
	for _, p := range peers {
		id, addr, ok := strings.Cut(p, "@")
		if !ok || !regexp.MustCompile(`^[0-9a-f]{40}$`).MatchString(id) || seen[id] || seenAddr[addr] {
			return nil, errors.New("PEER_REJECTED")
		}
		if e := loopback(addr); e != nil {
			return nil, e
		}
		seen[id] = true
		seenAddr[addr] = true
	}
	c := cmtcfg.DefaultConfig().SetRoot(o.home)
	c.RPC.ListenAddress = "tcp://" + o.rpc
	c.RPC.CORSAllowedOrigins = []string{}
	c.RPC.Unsafe = false
	c.RPC.MaxOpenConnections = 16
	c.RPC.MaxBodyBytes = 1 << 20
	c.P2P.ListenAddress = "tcp://" + o.p2p
	c.P2P.PersistentPeers = o.peers
	c.P2P.AddrBookStrict = false
	c.P2P.AllowDuplicateIP = true
	c.P2P.PexReactor = false
	c.P2P.Seeds = ""
	c.P2P.MaxNumInboundPeers = 3
	c.P2P.MaxNumOutboundPeers = 3
	c.Consensus.TimeoutCommit = time.Second
	c.Instrumentation.Prometheus = false
	c.RPC.PprofListenAddress = ""
	c.RPC.GRPCListenAddress = ""
	return c, c.ValidateBasic()
}
func validateHome(o options, in app.LocalDemoInputs, c *cmtcfg.Config) error {
	for _, p := range []string{o.home, filepath.Join(o.home, "config"), filepath.Join(o.home, "data")} {
		if e := safePath(p); e != nil {
			return e
		}
		s, e := os.Stat(p)
		if e != nil {
			return e
		}
		st, ok := s.Sys().(*syscall.Stat_t)
		if !ok || int(st.Uid) != os.Getuid() || !s.IsDir() || s.Mode().Perm() != 0700 {
			return errors.New("HOME_MODE_REQUIRED")
		}
	}
	for p, want := range map[string][]byte{c.GenesisFile(): in.Genesis, filepath.Join(o.home, "guard.dev.json"): in.Guard} {
		raw, e := readFile(p, 1<<20, false)
		if e != nil {
			return e
		}
		if !bytes.Equal(raw, want) {
			return errors.New("HOME_BINDING_MISMATCH")
		}
	}
	for _, p := range []string{c.PrivValidatorKeyFile(), c.PrivValidatorStateFile(), c.NodeKeyFile()} {
		raw, e := readFile(p, 1<<20, true)
		if e != nil {
			return e
		}
		if e = uniqueJSON(raw); e != nil {
			return e
		}
	}

	// Reject linked database paths on restart before any backend follows them.
	if e := filepath.WalkDir(filepath.Join(o.home, "data"), func(p string, d os.DirEntry, e error) error {
		if e != nil {
			return e
		}
		s, e := os.Lstat(p)
		if e != nil {
			return e
		}
		if s.IsDir() {
			return nil
		}
		st, ok := s.Sys().(*syscall.Stat_t)
		if !ok || !s.Mode().IsRegular() || st.Nlink != 1 {
			return errors.New("DATA_LINK_REJECTED")
		}
		return nil
	}); e != nil {
		return e
	}
	return validateKeys(c, in.Genesis)
}
func validateKeys(c *cmtcfg.Config, genesis []byte) (err error) {
	defer func() {
		if recover() != nil {
			err = errors.New("KEY_ENCODING_INVALID")
		}
	}()
	raw, e := readFile(c.PrivValidatorKeyFile(), 1<<20, true)
	if e != nil {
		return e
	}
	var k privval.FilePVKey
	if e = cmtjson.Unmarshal(raw, &k); e != nil {
		return errors.New("VALIDATOR_KEY_INVALID")
	}
	if k.PrivKey == nil || k.PubKey == nil || !k.PrivKey.PubKey().Equals(k.PubKey) || !bytes.Equal(k.Address, k.PubKey.Address()) {
		return errors.New("VALIDATOR_KEY_MISMATCH")
	}
	g, e := cmttypes.GenesisDocFromJSON(genesis)
	if e != nil {
		return e
	}
	found := false
	for _, v := range g.Validators {
		if v.PubKey.Equals(k.PubKey) {
			found = true
		}
	}
	if !found {
		return errors.New("VALIDATOR_NOT_IN_GENESIS")
	}
	raw, e = readFile(c.PrivValidatorStateFile(), 1<<20, true)
	if e != nil {
		return e
	}
	var state privval.FilePVLastSignState
	if e = cmtjson.Unmarshal(raw, &state); e != nil || state.Height < 0 || state.Round < 0 || state.Step < 0 || state.Step > 3 {
		return errors.New("VALIDATOR_STATE_INVALID")
	}
	if (len(state.Signature) == 0) != (len(state.SignBytes) == 0) {
		return errors.New("VALIDATOR_STATE_INCOMPLETE")
	}
	raw, e = readFile(c.NodeKeyFile(), 1<<20, true)
	if e != nil {
		return e
	}
	var nk p2p.NodeKey
	if e = cmtjson.Unmarshal(raw, &nk); e != nil || nk.PrivKey == nil {
		return errors.New("NODE_KEY_INVALID")
	}
	for _, peer := range strings.Split(c.P2P.PersistentPeers, ",") {
		if strings.HasPrefix(peer, string(nk.ID())+"@") {
			return errors.New("SELF_PEER_REJECTED")
		}
	}
	return nil
}
func lock(home string) (*os.File, error) {
	p := filepath.Join(home, "writer.dev.lock")
	fd, e := syscall.Open(p, syscall.O_RDWR|syscall.O_CREAT|syscall.O_NOFOLLOW, 0600)
	if e != nil {
		return nil, e
	}
	f := os.NewFile(uintptr(fd), p)
	fail := func(e error) (*os.File, error) { f.Close(); return nil, e }
	s, e := f.Stat()
	if e != nil {
		return fail(e)
	}
	st, ok := s.Sys().(*syscall.Stat_t)
	if !ok || !s.Mode().IsRegular() || s.Mode().Perm() != 0600 || st.Nlink != 1 {
		return fail(errors.New("WRITER_LOCK_METADATA"))
	}
	if e = syscall.Flock(fd, syscall.LOCK_EX|syscall.LOCK_NB); e != nil {
		return fail(errors.New("WRITER_ALREADY_ACTIVE"))
	}
	// Never unlink the lock: unlink/recreate permits two independent lock inodes.
	return f, nil
}
func run(args []string) error {
	if len(args) > 0 && args[0] == "topology" {
		return runTopology(args[1:], os.Stdout)
	}
	o, e := parse(args)
	if e != nil {
		return e
	}
	in, e := inputs(o)
	if e != nil {
		return e
	}
	c, e := config(o)
	if e != nil {
		return e
	}
	if e = validateHome(o, in, c); e != nil {
		return e
	}
	if o.mode == "preflight" {
		fmt.Println("VALIDATED_INPUT_BYTES_AND_HOME_ONLY; RUNTIME_APPROVAL_NOT_ESTABLISHED; durable_ack=false")
		return nil
	}
	l, e := lock(o.home)
	if e != nil {
		return e
	}
	defer l.Close()
	// Recheck the binding after obtaining the single-writer lock.
	if e = validateHome(o, in, c); e != nil {
		return e
	}
	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()
	if e = awaitStart(ctx, os.Stdin, os.Stdout, 30*time.Second); e != nil {
		return e
	}
	// Recheck mutable home files after the parent approval round trip.
	if e = validateHome(o, in, c); e != nil {
		return e
	}
	if ctx.Err() != nil {
		return errors.New("START_CANCELLED")
	}
	db, e := dbm.NewDB("application", dbm.GoLevelDBBackend, filepath.Join(o.home, "data"))
	if e != nil {
		return e
	}
	defer db.Close()
	a, e := app.NewLocalDemo(db, log.NewLogger(os.Stderr), in)
	if e != nil {
		return e
	}
	pv := privval.LoadFilePV(c.PrivValidatorKeyFile(), c.PrivValidatorStateFile())
	nk, e := p2p.LoadNodeKey(c.NodeKeyFile())
	if e != nil {
		return e
	}
	n, e := node.NewNode(c, pv, nk, proxy.NewLocalClientCreator(server.NewCometABCIWrapper(a)), node.DefaultGenesisDocProviderFunc(c), cmtcfg.DefaultDBProvider, node.DefaultMetricsProvider(c.Instrumentation), cmtlog.NewTMLogger(cmtlog.NewSyncWriter(os.Stderr)))
	if e != nil {
		return e
	}
	if ctx.Err() != nil { return errors.New("START_CANCELLED") }
	if e = n.Start(); e != nil {
		return e
	}
	<-ctx.Done()
	if e = n.Stop(); e != nil {
		return e
	}
	n.Wait()
	return nil
}
func main() {
	if e := run(os.Args[1:]); e != nil {
		fmt.Fprintln(os.Stderr, "nus-s3-local-chain:", e)
		os.Exit(1)
	}
}
