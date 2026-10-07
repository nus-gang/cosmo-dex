//go:build dev_local_demo

package localkeys

import (
	"encoding/json"
	"errors"
	app "github.com/nus-gang/cosmo-dex/chain/app"
)

// InputBundle encodes exact B-validated bytes for C's existing decode_bundle.
// It is an offline transport, not a runtime permit or bootstrap snapshot.
// Caller owns in exclusively for the duration of this call.
func InputBundle(in app.LocalDemoInputs) ([]byte, error) {
	if _, err := app.ValidateLocalDemo(in); err != nil {
		return nil, errors.New("INITIALIZATION_REJECTED")
	}
	raw, err := json.Marshal(struct {
		Manifest []byte            `json:"runtime_manifest"`
		Files    map[string][]byte `json:"files"`
		Guard    []byte            `json:"guard"`
		Genesis  []byte            `json:"genesis"`
	}{in.RuntimeManifest, in.Files, in.Guard, in.Genesis})
	if err != nil || len(raw) > 48*1024*1024 {
		return nil, errors.New("INITIALIZATION_REJECTED")
	}
	return raw, nil
}
