// S2 public fixture keys only. Does not implement a server or an SDK transaction.
package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
	"os"
)

func dec(s string) []byte {
	b, e := hex.DecodeString(s)
	if e != nil {
		panic(e)
	}
	return b
}
func main() {
	path := "../protocol/s2/vectors/signed.json"
	if len(os.Args) == 2 && os.Args[1] == "--keys" {
		keys := []map[string]string{}
		for n := 0; n < 2; n++ {
			var seed [32]byte
			for i := range seed {
				seed[i] = byte(i + n*32)
			}
			pk, _ := mldsa65.NewKeyFromSeed(&seed)
			pub, _ := pk.MarshalBinary()
			owner := sha256.Sum256(pub)
			keys = append(keys, map[string]string{"test_seed_hex": hex.EncodeToString(seed[:]), "public_key_hex": hex.EncodeToString(pub), "owner_raw_hex": hex.EncodeToString(owner[:20])})
		}
		out, _ := json.MarshalIndent(keys, "", "  ")
		if err := os.WriteFile("../protocol/s2/vectors/test-keys.json", append(out, '\n'), 0644); err != nil {
			panic(err)
		}
		fmt.Println("generated two public test keys; never use for assets")
		return
	}
	raw, e := os.ReadFile(path)
	if e != nil {
		panic(e)
	}
	var v map[string]interface{}
	if e = json.Unmarshal(raw, &v); e != nil {
		panic(e)
	}
	generate := len(os.Args) == 2 && os.Args[1] == "--generate"
	count := 0
	for _, item := range v["cases"].([]interface{}) {
		c := item.(map[string]interface{})
		var seed [32]byte
		copy(seed[:], dec(c["test_seed_hex"].(string)))
		pk, sk := mldsa65.NewKeyFromSeed(&seed)
		pub, _ := pk.MarshalBinary()
		if !bytes.Equal(pub, dec(c["public_key_hex"].(string))) {
			panic("key mismatch")
		}
		msg := dec(c["sign_input_hex"].(string))
		sig := make([]byte, mldsa65.SignatureSize)
		if e = mldsa65.SignTo(sk, msg, nil, false, sig); e != nil {
			panic(e)
		}
		if generate {
			c["signature_hex"] = hex.EncodeToString(sig)
		} else if !bytes.Equal(sig, dec(c["signature_hex"].(string))) {
			panic("fixture signature mismatch")
		}
		if !mldsa65.Verify(pk, msg, nil, sig) {
			panic("verify failed")
		}
		count++
		bad := append([]byte(nil), msg...)
		bad[len(bad)-1] ^= 1
		if mldsa65.Verify(pk, bad, nil, sig) {
			panic("mutation accepted")
		}
		count++
		h := sha256.Sum256(msg)
		if mldsa65.Verify(pk, h[:], nil, sig) {
			panic("prehash accepted")
		}
		count++
		if mldsa65.Verify(pk, msg, []byte("S2"), sig) {
			panic("nonempty context accepted")
		}
		count++
	}
	if generate {
		out, _ := json.MarshalIndent(v, "", "  ")
		if e = os.WriteFile(path, append(out, '\n'), 0644); e != nil {
			panic(e)
		}
	}
	fmt.Printf("PASS CIRCL ML-DSA-65: %d checks (per-case signature and mutation/prehash/context negatives); product Go/Rust/TS/SDK/runtime NOT_RUN\n", count)
}
