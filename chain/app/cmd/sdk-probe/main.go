// sdk-probe verifies the pinned SDK dependency surface; it is not nusd.
package main

import (
	"fmt"
	"github.com/cosmos/cosmos-sdk/baseapp"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
)

func main() {
	_ = baseapp.NewBaseApp
	key, err := mldsa65.GenPrivKeyFromSeed(make([]byte, 32))
	if err != nil {
		panic(err)
	}
	msg := []byte("NUS-19 dependency probe, not a transaction")
	sig, err := key.Sign(msg)
	if err != nil {
		panic(err)
	}
	fmt.Printf("SDK ML-DSA: public_key=%d signature=%d verified=%t\n", len(key.PubKey().Bytes()), len(sig), key.PubKey().VerifySignature(msg, sig))
}
