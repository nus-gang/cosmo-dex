//go:build dev_local_demo

package localkeys

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"

	app "github.com/nus-gang/cosmo-dex/chain/app"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
)

func digest(raw []byte) string { sum := sha256.Sum256(raw); return hex.EncodeToString(sum[:]) }

// PrepareGuard binds exact input bytes and delegates all runtime/genesis semantics
// to B. It creates no files/services and grants no organizational approval.
// in must be an exclusively owned captured input set; Guard must be empty.
// The returned guard still requires C validation before publication.
func PrepareGuard(in app.LocalDemoInputs, runUUID, feeProfile string) ([]byte, map[string]string, error) {
	reject := errors.New("INITIALIZATION_REJECTED")
	if len(in.Guard) != 0 || len(in.RuntimeManifest) > 2<<20 {
		return nil, nil, reject
	}
	var manifest struct {
		Contract  string `json:"contract_sha256"`
		Candidate string `json:"candidate_manifest_sha256"`
	}
	if json.Unmarshal(in.RuntimeManifest, &manifest) != nil {
		return nil, nil, reject
	}
	context := map[string]string{"service_schema": "s3/3", "chain_id": ex.S3ChainID, "genesis_hash": digest(in.Genesis), "contract_hash": manifest.Contract, "config_hash": digest(in.EffectiveProfile), "market_id": ex.S3Market, "market_config_version": "1"}
	guard, err := json.Marshal(map[string]any{"envelope_version": "s3-dev-local/1", "profile_id": "s3-dev-local-v1", "candidate_manifest_sha256": manifest.Candidate, "runtime_manifest_sha256": in.ApprovedRuntimeSHA256, "effective_profile_sha256": digest(in.EffectiveProfile), "run_uuid": runUUID, "fee_profile": feeProfile, "context": context})
	if err != nil {
		return nil, nil, reject
	}
	in.Guard = guard
	validated, err := app.ValidateLocalDemo(in)
	if err != nil {
		return nil, nil, reject
	}
	return guard, validated, nil
}
