// contract-runner exchanges one JSON object per line for cross-language S0 checks.
package main

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"

	"github.com/nus-gang/cosmo-dex/chain/contract"
)

type request struct {
	Op        string           `json:"op"`
	Message   string           `json:"message"`
	API       json.RawMessage  `json:"api_json"`
	Wire      string           `json:"wire_hex"`
	Domain    string           `json:"domain"`
	PublicKey string           `json:"public_key_hex"`
	Signature string           `json:"signature_hex"`
	SignInput string           `json:"sign_input_hex"`
	Context   contract.Context `json:"context"`
}

func run(r request) map[string]any {
	out := map[string]any{"code": "OK"}
	b, e := hex.DecodeString(r.Wire)
	if e == nil {
		switch r.Op {
		case "encode":
			b, e = contract.EncodeJSON(r.Message, r.API)
		case "decode":
			out["api_json"], e = contract.Decode(r.Message, b)
		case "verify":
			var sig []byte
			sig, e = hex.DecodeString(r.Signature)
			if e == nil {
				e = contract.Verify(r.Message, b, sig, r.Context)
			}
		case "verify_crypto":
			var pk, sig, msg []byte
			pk, e = hex.DecodeString(r.PublicKey)
			if e == nil {
				sig, e = hex.DecodeString(r.Signature)
			}
			if e == nil {
				msg, e = hex.DecodeString(r.SignInput)
			}
			if e == nil && !contract.VerifyCrypto(pk, msg, sig) {
				e = contract.Code("INVALID_SIGNATURE")
			}
		default:
			e = contract.Code("UNSUPPORTED_OPERATION")
		}
	}
	if e != nil {
		out["code"] = e.Error()
		return out
	}
	if r.Op == "encode" || r.Op == "decode" {
		out["wire_hex"] = hex.EncodeToString(b)
		if r.Domain != "" {
			f := contract.Frame(r.Domain, b)
			h := sha256.Sum256(f)
			out["sign_input_hex"] = hex.EncodeToString(f)
			out["sha256"] = hex.EncodeToString(h[:])
		}
	}
	return out
}
func main() {
	s := bufio.NewScanner(os.Stdin)
	s.Buffer(make([]byte, 4096), 5<<20)
	w := json.NewEncoder(os.Stdout)
	for s.Scan() {
		var r request
		if e := json.Unmarshal(s.Bytes(), &r); e != nil {
			_ = w.Encode(map[string]string{"code": "INVALID_JSON"})
			continue
		}
		_ = w.Encode(run(r))
	}
	if e := s.Err(); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
