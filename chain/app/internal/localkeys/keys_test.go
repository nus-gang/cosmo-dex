//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"testing"

	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
)

func TestFreshFourRoundTrip(t *testing.T) {
	seen := map[string]bool{}
	for profile := 0; profile < 2; profile++ {
		nodes, err := GenerateFour()
		if err != nil || len(nodes) != 4 {
			t.Fatal("generation failed")
		}
		for _, node := range nodes {
			public, id := node.Public()
			files := node.PrivateFiles()
			var pv privval.FilePVKey
			var state privval.FilePVLastSignState
			var nk p2p.NodeKey
			if cmtjson.Unmarshal(files["priv_validator_key.json"], &pv) != nil || cmtjson.Unmarshal(files["priv_validator_state.json"], &state) != nil || cmtjson.Unmarshal(files["node_key.json"], &nk) != nil {
				t.Fatal("Comet decode failed")
			}
			if !pv.PubKey.Equals(public.PubKey) || !bytes.Equal(pv.Address, public.Address) || !pv.PrivKey.PubKey().Equals(public.PubKey) || id != nk.ID() || public.Power != 10 {
				t.Fatal("public/private binding failed")
			}
			if state.Height != 0 || state.Round != 0 || state.Step != 0 || len(state.Signature) != 0 || len(state.SignBytes) != 0 {
				t.Fatal("nonfresh state")
			}
			for _, key := range []string{string(pv.PubKey.Bytes()), string(nk.PubKey().Bytes())} {
				if seen[key] {
					t.Fatal("reused identity")
				}
				seen[key] = true
			}
			message := []byte("synthetic verification only")
			sig, err := pv.PrivKey.Sign(message)
			if err != nil || !public.PubKey.VerifySignature(message, sig) {
				t.Fatal("signature roundtrip failed")
			}
		}
	}
}

type broken struct{}

func (broken) Read([]byte) (int, error) { return 0, errors.New("sensitive entropy detail") }
func TestEntropyFailureAndDuplicateReject(t *testing.T) {
	for _, source := range []struct {
		reader interface{ Read([]byte) (int, error) }
		code   string
	}{
		{broken{}, "KEY_ENTROPY_FAILED"}, {bytes.NewReader(make([]byte, 1024)), "DUPLICATE_KEY_REJECTED"},
	} {
		result, err := generate(source.reader)
		if result != nil || err == nil || err.Error() != source.code {
			t.Fatal("failure must return no material and fixed error")
		}
	}
}
func TestCopiesAndRedactedSerialization(t *testing.T) {
	nodes, err := GenerateFour()
	if err != nil {
		t.Fatal("generation failed")
	}
	node := nodes[0]
	before := node.PrivateFiles()
	changed := node.PrivateFiles()
	changed["node_key.json"][0] ^= 1
	delete(changed, "priv_validator_key.json")
	if !bytes.Equal(before["node_key.json"], node.PrivateFiles()["node_key.json"]) || len(node.PrivateFiles()) != 3 {
		t.Fatal("mutable private alias")
	}
	pub, _ := node.Public()
	pub.Address[0] ^= 1
	pub.PubKey.Bytes()[0] ^= 1
	after, _ := node.Public()
	if bytes.Equal(pub.Address, after.Address) || pub.PubKey.Equals(after.PubKey) {
		t.Fatal("mutable public alias")
	}
	if _, err := json.Marshal(node); err == nil {
		t.Fatal("private JSON allowed")
	}
	for _, format := range []string{"%v", "%+v", "%#v"} {
		if fmt.Sprintf(format, node) != "localkeys.Material(REDACTED)" {
			t.Fatal("log material leaked")
		}
	}
}
