//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"encoding/json"
	"errors"
	"time"

	cmtjson "github.com/cometbft/cometbft/libs/json"
	cmttypes "github.com/cometbft/cometbft/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
)

// GenesisBytes builds only the public consensus envelope. The caller must pass
// its result unchanged to approved B/C semantic validation before home publish.
// App state is opaque here: this function grants no runtime or economic approval.
func GenesisBytes(material []Material, at time.Time, appState []byte) ([]byte, error) {
	if len(material) != 4 || at.IsZero() || at.Location() != time.UTC || len(appState) == 0 || len(appState) > 900<<10 || !json.Valid(appState) || !bytes.HasPrefix(bytes.TrimSpace(appState), []byte("{")) {
		return nil, errors.New("GENESIS_INPUT_REJECTED")
	}
	validators := make([]cmttypes.GenesisValidator, 0, 4)
	seen := map[string]bool{}
	for _, m := range material {
		if m.validator.PubKey == nil || len(m.files) != 3 {
			return nil, errors.New("GENESIS_KEY_REJECTED")
		}
		v, peer := m.Public()
		key := string(v.PubKey.Bytes())
		if seen[key] || seen[string(peer)] || v.Power != 10 || !bytes.Equal(v.Address, v.PubKey.Address()) {
			return nil, errors.New("GENESIS_KEY_REJECTED")
		}
		seen[key], seen[string(peer)] = true, true
		validators = append(validators, v)
	}
	params := cmttypes.DefaultConsensusParams()
	params.Block.MaxBytes = 1048576
	params.Block.MaxGas = 20000000
	params.Evidence.MaxBytes = 65536
	doc := cmttypes.GenesisDoc{GenesisTime: at, ChainID: ex.S3ChainID, InitialHeight: 1, ConsensusParams: params, Validators: validators, AppState: append(json.RawMessage(nil), appState...)}
	if err := doc.ValidateAndComplete(); err != nil {
		return nil, errors.New("GENESIS_CONSENSUS_REJECTED")
	}
	raw, err := cmtjson.Marshal(doc)
	if err != nil || len(raw) > 1<<20 {
		return nil, errors.New("GENESIS_ENCODING_REJECTED")
	}
	return raw, nil
}
