//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
)

// ParseUserPublicKeys accepts only the public half exported by two fresh
// browser-held ML-DSA-65 keys. Private user material must never cross this API.
func ParseUserPublicKeys(raw []byte) ([][]byte, error) {
	reject := errors.New("USER_KEYS_REJECTED")
	if len(raw) == 0 || len(raw) > 16*1024 {
		return nil, reject
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	var encoded []string
	if err := decoder.Decode(&encoded); err != nil || len(encoded) != 2 {
		return nil, reject
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return nil, reject
	}
	out := make([][]byte, 2)
	seen := map[string]bool{}
	for i, value := range encoded {
		decoded, err := base64.StdEncoding.Strict().DecodeString(value)
		if err != nil || len(decoded) != 1952 || base64.StdEncoding.EncodeToString(decoded) != value || seen[string(decoded)] {
			for _, key := range out {
				clear(key)
			}
			return nil, reject
		}
		seen[string(decoded)] = true
		out[i] = decoded
	}
	return out, nil
}
