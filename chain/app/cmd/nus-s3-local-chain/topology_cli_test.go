//go:build dev_local_demo

package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func topologyArgv(t *testing.T) [][]string {
	opts, _, _ := topologyFixture(t)
	out := make([][]string, 4)
	for i, o := range opts {
		out[i] = []string{"preflight", "--local-demo-profile", "/private/profile", "--acknowledge-unproven-space", "--input-set", "/private/input", "--runtime-pin", strings.Repeat("a", 64), "--home", o.home, "--rpc", o.rpc, "--p2p", o.p2p, "--peers", o.peers}
	}
	return out
}
func TestTopologyCommandArguments(t *testing.T) {
	a := topologyArgv(t)
	raw, _ := json.Marshal(a)
	opts, e := topologyOptions(raw)
	if e != nil || len(opts) != 4 {
		t.Fatal(e)
	}
	for _, which := range []string{"start", "pin", "profile", "count", "duplicate", "external"} {
		t.Run(which, func(t *testing.T) {
			var b [][]string
			json.Unmarshal(raw, &b)
			switch which {
			case "start":
				b[0][0] = "start"
			case "pin":
				b[1][8] = strings.Repeat("b", 64)
			case "profile":
				b[1][2] = "/different"
			case "count":
				b = b[:3]
			case "duplicate":
				b[0] = append(b[0], "--home", "/x")
			case "external":
				b[0][12] = "0.0.0.0:1234"
			}
			bad, _ := json.Marshal(b)
			if _, e := topologyOptions(bad); e == nil {
				t.Fatal("accepted")
			}
		})
	}
	for _, b := range [][]byte{[]byte("null"), []byte("[] []"), bytes.Repeat([]byte(" "), 65537)} {
		if _, e := topologyOptions(b); e == nil {
			t.Fatal("shape accepted")
		}
	}
}
func TestTopologyCommandRefusalNoOutput(t *testing.T) {
	root := privateDir(t)
	p := filepath.Join(root, "topology.json")
	a := topologyArgv(t)
	raw, _ := json.Marshal(a)
	if e := os.WriteFile(p, raw, 0600); e != nil {
		t.Fatal(e)
	}
	for _, args := range [][]string{nil, {"--topology", p, "extra"}, {"--topology", p}, {"--topology", "relative"}} {
		var out bytes.Buffer
		if e := runTopology(args, &out); e == nil || out.Len() != 0 {
			t.Fatal("invalid candidate accepted or output leaked")
		}
	}
	after, e := os.ReadFile(p)
	if e != nil || !bytes.Equal(raw, after) {
		t.Fatal("input changed")
	}
}
