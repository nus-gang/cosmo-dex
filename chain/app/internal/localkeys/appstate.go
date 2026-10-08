//go:build dev_local_demo

package localkeys

import (
	"encoding/hex"
	"encoding/json"
	"errors"

	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	app "github.com/nus-gang/cosmo-dex/chain/app"
)

// AppStateBytes encodes public registration inputs with B's exact genesis type.
// Hashes must come from the captured runtime/profile. This bounded encoder does
// not approve those hashes, prove key possession or initialize any chain state.
// The completed genesis and guard MUST pass ValidateLocalDemo and C validation
// before publication. No fixture key or standard-profile hash is substituted.
func AppStateBytes(users, operators [][]byte, admin []byte, fee, contractHash, configHash string) ([]byte, error) {
	reject := errors.New("REGISTRATION_REJECTED")
	if len(users) < 2 || len(users) > 16 || len(operators) != 2 || (fee != "0" && fee != "25") {
		return nil, reject
	}
	for _, value := range []string{contractHash, configHash} {
		raw, err := hex.DecodeString(value)
		if err != nil || len(raw) != 32 || hex.EncodeToString(raw) != value {
			return nil, reject
		}
	}
	seen := map[string]bool{}
	for _, group := range [][][]byte{users, operators, {admin}} {
		for _, key := range group {
			if len(key) != 1952 {
				return nil, reject
			}
			address := string((&mldsa65.PubKey{Key: key}).Address())
			if seen[address] {
				return nil, reject
			}
			seen[address] = true
		}
	}
	raw, err := json.Marshal(app.S3Genesis{PublicKeys: users, OperatorKeys: operators, AdminKey: admin, FeeBPS: fee, ContractHash: contractHash, ConfigHash: configHash})
	if err != nil {
		return nil, reject
	}
	return raw, nil
}
