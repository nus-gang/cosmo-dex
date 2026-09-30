package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
)

// This input contains only public keys. Never echo malformed input in errors.
func readUserPublicKeys(path string) ([][]byte, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("cannot read user public keys file")
	}
	var encoded []string
	if err := json.Unmarshal(raw, &encoded); err != nil || len(encoded) != 2 {
		return nil, fmt.Errorf("user public keys must be a JSON array of exactly two strings")
	}
	keys := make([][]byte, 2)
	for i, value := range encoded {
		key, err := base64.StdEncoding.Strict().DecodeString(value)
		if err != nil || len(key) != 1952 || base64.StdEncoding.EncodeToString(key) != value {
			return nil, fmt.Errorf("user public key %d must be canonical base64 of 1952 bytes", i)
		}
		keys[i] = key
	}
	if encoded[0] == encoded[1] {
		return nil, fmt.Errorf("duplicate user public keys")
	}
	return keys, nil
}
