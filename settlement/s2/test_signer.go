// Test-only signer: the two public nusd development seeds, never production keys.
// Build from chain/: go build -o <path> ../settlement/s2/test_signer.go
package main

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
	"github.com/nus-gang/cosmo-dex/chain/contract"
	"os"
)

func must(err error) {
	if err != nil {
		panic(err)
	}
}
func main() {
	var in struct {
		User   int            `json:"user"`
		Type   string         `json:"type"`
		Fields map[string]any `json:"fields"`
		Wire   string         `json:"wire_base64"`
	}
	must(json.NewDecoder(os.Stdin).Decode(&in))
	if in.User < 0 || in.User > 1 {
		panic("public test user only")
	}
	var seed [32]byte
	seed[0] = byte(in.User + 1)
	pk, sk := mldsa65.NewKeyFromSeed(&seed)
	pub, err := pk.MarshalBinary()
	must(err)
	owner := sha256.Sum256(pub)
	b64 := base64.StdEncoding.EncodeToString
	out := map[string]any{"owner": b64(owner[:20]), "public_key": b64(pub)}
	if in.Type != "identity" {
		domains := map[string]string{"OrderV1": "NUS/ORDER/V1", "CancelV1": "NUS/CANCEL/V1", "WalletChallengeV1": "NUS/WALLET_AUTH/V1"}
		domain, ok := domains[in.Type]
		if !ok {
			panic("unsupported type")
		}
		var wire []byte
		if in.Wire != "" {
			wire, err = base64.StdEncoding.DecodeString(in.Wire)
		} else {
			wire, err = contract.Encode(in.Type, in.Fields)
		}
		must(err)
		signature := make([]byte, mldsa65.SignatureSize)
		framed := contract.Frame(domain, wire)
		must(mldsa65.SignTo(sk, framed, nil, false, signature))
		hash := sha256.Sum256(framed)
		out["wire_base64"] = b64(wire)
		out["signature_base64"] = b64(signature)
		out["hash"] = hex.EncodeToString(hash[:])
	}
	must(json.NewEncoder(os.Stdout).Encode(out))
}
