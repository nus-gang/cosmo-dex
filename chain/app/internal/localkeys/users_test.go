//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"testing"
)

func TestParseUserPublicKeys(t *testing.T) {
	keys := registrationKeys(t)[:2]
	encoded := []string{base64.StdEncoding.EncodeToString(keys[0]), base64.StdEncoding.EncodeToString(keys[1])}
	raw, _ := json.Marshal(encoded)
	got, err := ParseUserPublicKeys(raw)
	if err != nil || !bytes.Equal(got[0], keys[0]) || !bytes.Equal(got[1], keys[1]) {
		t.Fatal("valid public keys rejected")
	}
	got[0][0] ^= 1
	if bytes.Equal(got[0], keys[0]) {
		t.Fatal("unexpected input alias")
	}
	for _, bad := range [][]byte{
		nil, []byte(`null`), []byte(`[]`), []byte(`{}`), []byte(`[` + `"` + encoded[0] + `"]`),
		[]byte(`[` + `"` + encoded[0] + `","` + encoded[0] + `"]`),
		[]byte(`[` + `"` + encoded[0][:len(encoded[0])-2] + `B=","` + encoded[1] + `"]`),
		append(raw, []byte(` {}`)...),
	} {
		if parsed, err := ParseUserPublicKeys(bad); err == nil || parsed != nil {
			t.Fatal("invalid public keys accepted")
		}
	}
}
