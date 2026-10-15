// Public S3 fixture material only; never load these seeds into a runtime.
package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"sort"
	"strings"
	"time"

	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

func dec(s string) []byte {
	b, e := hex.DecodeString(s)
	if e != nil {
		panic(e)
	}
	return b
}
func save(path string, v any) {
	b, e := json.MarshalIndent(v, "", "  ")
	if e != nil {
		panic(e)
	}
	if e = os.WriteFile(path, append(b, '\n'), 0644); e != nil {
		panic(e)
	}
}
func main() {
	if len(os.Args) < 3 {
		panic("usage: crypto.go keys|sign|check S3_DIR")
	}
	dir := os.Args[2]
	if os.Args[1] == "keys" {
		keys := []map[string]string{}
		for n := 0; n < 19; n++ {
			seed := sha256.Sum256([]byte(fmt.Sprintf("NUS-54 S3 PUBLIC FIXTURE %02d 2026-10-05", n)))
			pk, _ := mldsa65.NewKeyFromSeed(&seed)
			pub, _ := pk.MarshalBinary()
			owner := sha256.Sum256(pub)
			keys = append(keys, map[string]string{"id": fmt.Sprintf("K%02d", n), "test_seed_hex": hex.EncodeToString(seed[:]), "public_key_hex": hex.EncodeToString(pub), "owner_raw_hex": hex.EncodeToString(owner[:20])})
		}
		save(dir+"/vectors/test-keys.json", keys)
		fmt.Println("PASS generated 19 fresh public fixture keys; runtime use forbidden")
		return
	}
	b, e := os.ReadFile(dir + "/vectors/signed.json")
	if e != nil {
		panic(e)
	}
	var v map[string]any
	if e = json.Unmarshal(b, &v); e != nil {
		panic(e)
	}
	count := 0
	var benchPK *mldsa65.PublicKey
	var benchMsg, benchSig []byte
	for _, item := range v["cases"].([]any) {
		c := item.(map[string]any)
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
		if os.Args[1] == "sign" {
			c["signature_hex"] = hex.EncodeToString(sig)
		} else if !bytes.Equal(sig, dec(c["signature_hex"].(string))) {
			panic("signature mismatch")
		}
		if !mldsa65.Verify(pk, msg, nil, sig) {
			panic("signature failure")
		}
		count++
		bad := append([]byte(nil), msg...)
		bad[len(bad)-1] ^= 1
		if mldsa65.Verify(pk, bad, nil, sig) {
			panic("mutation accepted")
		}
		count++
		h := sha256.Sum256(msg)
		if mldsa65.Verify(pk, h[:], nil, sig) || mldsa65.Verify(pk, msg, []byte("S3"), sig) {
			panic("domain confusion")
		}
		count += 2
		if c["kind"] == "OrderV1" {
			raw := dec(c["canonical_hex"].(string))
			fields, er := contract.Decode("OrderV1", raw)
			if er != nil {
				panic(er)
			}
			re, er := contract.Encode("OrderV1", fields)
			if er != nil || !bytes.Equal(re, raw) {
				panic("v1 roundtrip")
			}
			count++
			if strings.HasPrefix(c["id"].(string), "demo-") || strings.HasPrefix(c["id"].(string), "capacity-valid-") {
				bps := uint64(0)
				if strings.HasPrefix(c["id"].(string), "demo-25-") {
					bps = 25
				}
				ctx := contract.Context{ChainID: "nus-s3-dev-1", GenesisHash: fields["genesis_hash"].(string), ModuleID: "x/exchange", MarketID: "DEVBASE/DEVQUOTE", MarketConfigVersion: "1", RegisteredKey: pub, RegisteredKeyType: "ML-DSA-65", Height: 199, Epoch: 0, MaxPrice: 1000000, MaxQuantity: 1000000, ActiveFeeBPS: bps}
				if er = contract.Verify("OrderV1", raw, sig, ctx); er != nil {
					panic(er)
				}
				count++
				ctx.Height = 200
				if er = contract.Verify("OrderV1", raw, sig, ctx); er == nil || er.Error() != "EXPIRED" {
					panic("expiry boundary")
				}
				count++
				ctx.Height = 199
				ctx.GenesisHash = strings.Repeat("00", 32)
				if contract.Verify("OrderV1", raw, sig, ctx) == nil {
					panic("wrong genesis accepted")
				}
				count++
			}
			benchPK, benchMsg, benchSig = pk, msg, sig
		}
	}
	if os.Args[1] == "sign" {
		save(dir+"/vectors/signed.json", v)
	}
	if os.Args[1] == "check" {
		b, e = os.ReadFile(dir + "/vectors/batches.json")
		if e != nil {
			panic(e)
		}
		var batches []map[string]any
		if e = json.Unmarshal(b, &batches); e != nil {
			panic(e)
		}
		for _, c := range batches {
			raw := dec(c["canonical_hex"].(string))
			m, er := contract.Decode("BatchV1", raw)
			if er != nil {
				panic(er)
			}
			re, er := contract.Encode("BatchV1", m)
			if er != nil || !bytes.Equal(raw, re) {
				panic("batch roundtrip")
			}
			count++
		}
		fmt.Printf("PASS S3 crypto + inherited Go strict codec: %d checks; SDK execution/Rust/TS product NOT_RUN\n", count)
		return
	}
	if benchPK != nil {
		for i := 0; i < 100; i++ {
			if !mldsa65.Verify(benchPK, benchMsg, nil, benchSig) {
				panic("warmup")
			}
		}
		ns := make([]int64, 1000)
		for i := range ns {
			start := time.Now()
			for j := 0; j < 34; j++ {
				if !mldsa65.Verify(benchPK, benchMsg, nil, benchSig) {
					panic("bench")
				}
			}
			ns[i] = time.Since(start).Nanoseconds()
		}
		sorted := append([]int64(nil), ns...)
		sort.Slice(sorted, func(i, j int) bool { return sorted[i] < sorted[j] })
		save(dir+"/evidence/crypto-benchmark.json", map[string]any{"scope": "CIRCL pure verification microbenchmark; not block gas or consensus latency", "go": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH, "circl": "v1.6.3", "warmups": 100, "samples": 1000, "verifications_per_sample": 34, "sample_nanoseconds": ns, "p50_ns": sorted[499], "p95_ns": sorted[949], "p99_ns": sorted[989], "max_ns": sorted[999], "message_sha256": fmt.Sprintf("%x", sha256.Sum256(benchMsg))})
	}
	fmt.Printf("PASS S3 signatures: %d checks; benchmark recorded separately\n", count)
}
