//go:build dev_local_demo

package localkeys

import (
	"bytes"
	cmttypes "github.com/cometbft/cometbft/types"
	"testing"
	"time"
)

func TestGenesisEnvelope(t *testing.T) {
	at := time.Unix(1800000000, 0).UTC()
	var previous []byte
	for _, state := range []string{`{"fee_bps":0}`, `{"fee_bps":25}`} {
		keys, err := GenerateFour()
		if err != nil {
			t.Fatal(err)
		}
		raw, err := GenesisBytes(keys, at, []byte(state))
		if err != nil {
			t.Fatal(err)
		}
		again, err := GenesisBytes(keys, at, []byte(state))
		if err != nil || !bytes.Equal(raw, again) {
			t.Fatal("unstable envelope")
		}
		doc, err := cmttypes.GenesisDocFromJSON(raw)
		if err != nil {
			t.Fatal(err)
		}
		if doc.ChainID != "nus-s3-dev-1" || doc.InitialHeight != 1 || len(doc.Validators) != 4 || !bytes.Equal(doc.AppState, []byte(state)) || !doc.GenesisTime.Equal(at) {
			t.Fatal("envelope mismatch")
		}
		if doc.ConsensusParams.Block.MaxBytes != 1048576 || doc.ConsensusParams.Block.MaxGas != 20000000 || doc.ConsensusParams.Evidence.MaxBytes != 65536 {
			t.Fatal("caps")
		}
		for i, key := range keys {
			v, _ := key.Public()
			if !bytes.Equal(v.PubKey.Bytes(), doc.Validators[i].PubKey.Bytes()) {
				t.Fatal("key mismatch")
			}
			for _, secret := range key.PrivateFiles() {
				if bytes.Contains(raw, secret) {
					t.Fatal("private material")
				}
			}
		}
		if bytes.Equal(previous, raw) {
			t.Fatal("profiles reused")
		}
		previous = raw
		raw[0] = '!'
		if _, err := GenesisBytes(keys, at, []byte(state)); err != nil {
			t.Fatal("output alias")
		}
	}
}

func TestGenesisEnvelopeRejects(t *testing.T) {
	keys, err := GenerateFour()
	if err != nil {
		t.Fatal(err)
	}
	at := time.Unix(1800000000, 0).UTC()
	for _, raw := range [][]byte{nil, []byte(`[]`), []byte(`null`), []byte(`{`), bytes.Repeat([]byte(" "), 900<<10+1)} {
		if got, err := GenesisBytes(keys, at, raw); err == nil || got != nil {
			t.Fatal("invalid state accepted")
		}
	}
	for _, set := range [][]Material{nil, keys[:3], {keys[0], keys[0], keys[2], keys[3]}, {{}, keys[1], keys[2], keys[3]}} {
		if got, err := GenesisBytes(set, at, []byte(`{}`)); err == nil || got != nil {
			t.Fatal("invalid keys accepted")
		}
	}
	for _, stamp := range []time.Time{{}, at.In(time.FixedZone("other", 3600))} {
		if got, err := GenesisBytes(keys, stamp, []byte(`{}`)); err == nil || got != nil {
			t.Fatal("invalid time accepted")
		}
	}
}
