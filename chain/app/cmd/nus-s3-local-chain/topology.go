//go:build dev_local_demo

package main

import (
	"bytes"
	"errors"
	"fmt"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
	cmttypes "github.com/cometbft/cometbft/types"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	"strings"
)

// inspectHomeTopology is a read-only, point-in-time identity check. Callers must
// first validate B inputs and retain the private-home/no-concurrent-writer
// assumption. It neither acquires writer locks nor grants launch permission.
// Only public identities escape; private key bytes never enter the report.
func inspectHomeTopology(opts []options, in app.LocalDemoInputs) (ids []string, err error) {
	defer func() {
		if recover() != nil {
			ids = nil
			err = errors.New("HOME_TOPOLOGY_REJECTED")
		}
	}()
	reject := func() ([]string, error) { return nil, errors.New("HOME_TOPOLOGY_REJECTED") }
	if len(opts) != 4 {
		return reject()
	}
	g, e := cmttypes.GenesisDocFromJSON(in.Genesis)
	if e != nil || len(g.Validators) != 4 {
		return reject()
	}
	validators := map[string]bool{}
	for _, v := range g.Validators {
		if v.PubKey == nil || !bytes.Equal(v.Address, v.PubKey.Address()) || v.Power != 10 {
			return reject()
		}
		key := string(v.PubKey.Bytes())
		if validators[key] {
			return reject()
		}
		validators[key] = true
	}
	ids = make([]string, 4)
	homes, ports, seenIDs, seenValidators := map[string]bool{}, map[string]bool{}, map[string]bool{}, map[string]bool{}
	for i, o := range opts {
		if homes[o.home] || ports[o.rpc] || ports[o.p2p] {
			return reject()
		}
		homes[o.home] = true
		ports[o.rpc] = true
		ports[o.p2p] = true
		c, e := config(o)
		if e != nil {
			return reject()
		}
		if e = validateHome(o, in, c); e != nil {
			return reject()
		}
		raw, e := readFile(c.NodeKeyFile(), 1<<20, true)
		if e != nil {
			return reject()
		}
		var nk p2p.NodeKey
		if uniqueJSON(raw) != nil || cmtjson.Unmarshal(raw, &nk) != nil || nk.PrivKey == nil {
			return reject()
		}
		ids[i] = string(nk.ID())
		if seenIDs[ids[i]] {
			return reject()
		}
		seenIDs[ids[i]] = true
		raw, e = readFile(c.PrivValidatorKeyFile(), 1<<20, true)
		if e != nil {
			return reject()
		}
		var pv privval.FilePVKey
		if uniqueJSON(raw) != nil || cmtjson.Unmarshal(raw, &pv) != nil || pv.PubKey == nil || pv.PrivKey == nil || !pv.PrivKey.PubKey().Equals(pv.PubKey) || !bytes.Equal(pv.Address, pv.PubKey.Address()) {
			return reject()
		}
		key := string(pv.PubKey.Bytes())
		if !validators[key] || seenValidators[key] {
			return reject()
		}
		seenValidators[key] = true
	}
	for i, o := range opts {
		want := map[string]bool{}
		for j, other := range opts {
			if i != j {
				want[fmt.Sprintf("%s@%s", ids[j], other.p2p)] = true
			}
		}
		for _, peer := range strings.Split(o.peers, ",") {
			if !want[peer] {
				return reject()
			}
			delete(want, peer)
		}
		if len(want) != 0 {
			return reject()
		}
	}
	return ids, nil
}
