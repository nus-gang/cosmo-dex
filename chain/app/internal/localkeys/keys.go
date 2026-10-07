//go:build dev_local_demo

// Package localkeys prepares ephemeral Comet and ML-DSA authority material
// and public genesis bytes. It does not publish files or start services.
package localkeys

import (
	"crypto/ed25519"
	"crypto/rand"
	"errors"
	"fmt"
	"io"

	cmted "github.com/cometbft/cometbft/crypto/ed25519"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
	cmttypes "github.com/cometbft/cometbft/types"
)

// Material keeps private serialized bytes out of default JSON and log formatting.
// PrivateFiles returns copies for the private home publisher; never log them.
type Material struct {
	validator cmttypes.GenesisValidator
	peerID    p2p.ID
	files     map[string][]byte
}

func (Material) String() string   { return "localkeys.Material(REDACTED)" }
func (Material) GoString() string { return "localkeys.Material(REDACTED)" }
func (Material) MarshalJSON() ([]byte, error) {
	return nil, errors.New("PRIVATE_MATERIAL_SERIALIZATION_REJECTED")
}
func (m Material) Public() (cmttypes.GenesisValidator, p2p.ID) {
	v := m.validator
	v.Address = append([]byte(nil), v.Address...)
	v.PubKey = cmted.PubKey(append([]byte(nil), v.PubKey.Bytes()...))
	return v, m.peerID
}
func (m Material) PrivateFiles() map[string][]byte {
	out := make(map[string][]byte, len(m.files))
	for k, v := range m.files {
		out[k] = append([]byte(nil), v...)
	}
	return out
}

// GenerateFour has no seed or fixture input. Every call uses fresh OS entropy.
// A caller must generate separately for fee0 and fee25, then validate genesis
// with the approved B/C APIs before publishing any home.
func GenerateFour() ([]Material, error) { return generate(rand.Reader) }
func generate(entropy io.Reader) ([]Material, error) {
	out := make([]Material, 0, 4)
	seen := map[string]bool{}
	for i := 0; i < 4; i++ {
		keys := make([]cmted.PrivKey, 2)
		for j := range keys {
			_, key, err := ed25519.GenerateKey(entropy)
			if err != nil {
				return nil, errors.New("KEY_ENTROPY_FAILED")
			}
			keys[j] = cmted.PrivKey(key)
			pub := string(keys[j].PubKey().Bytes())
			if seen[pub] {
				return nil, errors.New("DUPLICATE_KEY_REJECTED")
			}
			seen[pub] = true
		}
		pv := privval.NewFilePV(keys[0], "", "")
		nk := p2p.NodeKey{PrivKey: keys[1]}
		files := map[string][]byte{}
		for name, value := range map[string]any{
			"priv_validator_key.json":   pv.Key,
			"priv_validator_state.json": pv.LastSignState,
			"node_key.json":             nk,
		} {
			raw, err := cmtjson.Marshal(value)
			if err != nil {
				return nil, errors.New("KEY_ENCODING_FAILED")
			}
			files[name] = raw
		}
		out = append(out, Material{
			validator: cmttypes.GenesisValidator{Address: keys[0].PubKey().Address(), PubKey: keys[0].PubKey(), Power: 10, Name: fmt.Sprintf("local-validator-%d", i+1)},
			peerID:    nk.ID(), files: files,
		})
	}
	return out, nil
}
