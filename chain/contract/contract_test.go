package contract

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"os"
	"strconv"
	"strings"
	"testing"

	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
)

func fixture(t *testing.T, name string) map[string]any {
	t.Helper()
	b, e := os.ReadFile("../../protocol/v1/vectors/" + name + ".json")
	if e != nil {
		t.Fatal(e)
	}
	var x map[string]any
	if e = json.Unmarshal(b, &x); e != nil {
		t.Fatal(e)
	}
	return x
}
func hx(s any) []byte {
	b, e := hex.DecodeString(s.(string))
	if e != nil {
		panic(e)
	}
	return b
}
func cases(x map[string]any, k string) []any { return x[k].([]any) }
func code(e error) string {
	if e == nil {
		return "OK"
	}
	return e.Error()
}
func fieldsAPI(t *testing.T, name string, rows []any) map[string]any {
	t.Helper()
	m := map[string]any{}
	for _, f := range schema[name] {
		if f.Repeated {
			m[f.Name] = []any{}
		}
	}
	for _, row := range rows {
		a := row.([]any)
		tag := uint64(a[0].(float64))
		var f field
		for _, s := range schema[name] {
			if s.Tag == tag {
				f = s
				break
			}
		}
		if f.Name == "" {
			t.Fatal("unknown fixture tag")
		}
		v := a[2]
		if a[1] == "hex" {
			b := hx(v)
			switch f.Type {
			case "h":
			case "atoms":
				v, _ = AtomsDecimal(b)
			default:
				v = base64.StdEncoding.EncodeToString(b)
			}
		}
		m[f.Name] = v
	}
	return m
}
func TestSignatureVectors(t *testing.T) {
	x := fixture(t, "signatures")
	names := map[string]string{"order": "OrderV1", "cancel": "CancelV1", "wallet": "WalletChallengeV1"}
	for _, v := range cases(x, "positives") {
		v := v.(map[string]any)
		t.Run(v["id"].(string), func(t *testing.T) {
			name := names[v["id"].(string)]
			m := fieldsAPI(t, name, v["fields"].([]any))
			b, e := Encode(name, m)
			if e != nil || !bytes.Equal(b, hx(v["canonical_hex"])) {
				t.Fatalf("encode %v", e)
			}
			f := Frame(string(hx(v["domain_hex"])), b)
			if !bytes.Equal(f, hx(v["sign_input_hex"])) {
				t.Fatal("frame")
			}
			h := sha256.Sum256(f)
			if hex.EncodeToString(h[:]) != v["sha256"] {
				t.Fatal("hash")
			}
			pk, sig := hx(v["public_key_hex"]), hx(v["signature_hex"])
			if !VerifyCrypto(pk, f, sig) {
				t.Fatal("real ML-DSA verification failed")
			}
			owner, e := Address(pk)
			if e != nil || !bytes.Equal(owner, hx(v["owner_raw_hex"])) {
				t.Fatal("address")
			}
			var seed [32]byte
			copy(seed[:], hx(x["test_seed_hex"]))
			pub, priv := mldsa65.NewKeyFromSeed(&seed)
			generated := make([]byte, mldsa65.SignatureSize)
			if e = mldsa65.SignTo(priv, f, nil, false, generated); e != nil {
				t.Fatal(e)
			}
			if !bytes.Equal(pub.Bytes(), pk) || !bytes.Equal(generated, sig) {
				t.Fatal("cross implementation deterministic signature mismatch")
			}
		})
	}
	for _, v := range cases(x, "negatives") {
		v := v.(map[string]any)
		t.Run(v["id"].(string), func(t *testing.T) {
			pk, msg, sig := hx(v["public_key_hex"]), hx(v["message_hex"]), hx(v["signature_hex"])
			valid := false
			if len(hx(v["context_hex"])) == 0 {
				valid = VerifyCrypto(pk, msg, sig)
			} else {
				var key mldsa65.PublicKey
				if key.UnmarshalBinary(pk) == nil {
					valid = mldsa65.Verify(&key, msg, hx(v["context_hex"]), sig)
				}
			}
			if valid != v["expected_crypto_valid"].(bool) {
				t.Fatal("unexpected crypto verdict")
			}
		})
	}
}
func TestWireVectors(t *testing.T) {
	for _, file := range []string{"wire-cases", "message-codec"} {
		x := fixture(t, file)
		key := "cases"
		if file == "message-codec" {
			key = "wire_cases"
		}
		for _, v := range cases(x, key) {
			v := v.(map[string]any)
			t.Run(v["id"].(string), func(t *testing.T) {
				b := hx(v["wire_hex"])
				m, e := Decode(v["message"].(string), b)
				got := "CANONICAL"
				if e != nil {
					got = "NON_CANONICAL_WIRE"
				}
				if got != v["expected"] {
					t.Fatalf("%s: %v", got, e)
				}
				if e == nil {
					out, e := Encode(v["message"].(string), m)
					if e != nil || !bytes.Equal(b, out) {
						t.Fatal("roundtrip")
					}
				}
			})
		}
	}
}
func TestMessageVectors(t *testing.T) {
	for _, v := range cases(fixture(t, "message-codec"), "positives") {
		v := v.(map[string]any)
		t.Run(v["id"].(string), func(t *testing.T) {
			raw, _ := json.Marshal(v["api_json"])
			b, e := EncodeJSON(v["message"].(string), raw)
			if e != nil || !bytes.Equal(b, hx(v["canonical_hex"])) {
				t.Fatalf("codec %v", e)
			}
			if f, ok := v["payment_frame_hex"]; ok {
				frame := Frame("NUS/PAYMENT_ID/V1", b)
				if !bytes.Equal(frame, hx(f)) {
					t.Fatal("payment frame")
				}
				h := sha256.Sum256(frame)
				if hex.EncodeToString(h[:]) != v["payment_hash"] {
					t.Fatal("payment hash")
				}
			}
		})
	}
}
func TestAmountVectors(t *testing.T) {
	for i, v := range cases(fixture(t, "amount-codec"), "cases") {
		v := v.(map[string]any)
		t.Run(strconv.Itoa(i), func(t *testing.T) {
			var e error
			if val, ok := v["api_json"]; ok {
				s, ok := val.(string)
				if !ok {
					e = IntegerRange
				} else {
					var b []byte
					b, e = Atoms(s)
					if e == nil {
						if !bytes.Equal(b, hx(v["wire_hex"])) {
							t.Fatal("bytes")
						}
						back, err := AtomsDecimal(b)
						if err != nil || back != s {
							t.Fatal("roundtrip")
						}
					}
				}
			} else {
				_, e = AtomsDecimal(hx(v["wire_hex"]))
			}
			if code(e) != v["expected"] {
				t.Fatalf("got %v", e)
			}
		})
	}
}
func TestPolicies(t *testing.T) {
	for _, v := range cases(fixture(t, "policy-cases"), "cases") {
		v := v.(map[string]any)
		t.Run(v["id"].(string), func(t *testing.T) {
			n := func(k string) uint64 {
				x, e := strconv.ParseUint(v[k].(string), 10, 64)
				if e != nil {
					t.Fatal(e)
				}
				return x
			}
			var e error
			if v["kind"] == "order_and_cancel_expiry" {
				e = Expiry(n("height"), n("expiry_height"))
			} else {
				e = WalletPolicy(n("issued_at"), n("expiry_time"), n("now"), v["origin"].(string), v["allowed_origin"].(string), v["audience"].(string), "private-ws", v["nonce_consumed"].(bool))
			}
			if (e == nil) != (v["expected"] == "allow") {
				t.Fatalf("%v", e)
			}
		})
	}
}
func TestOrderValidation(t *testing.T) {
	x := fixture(t, "signatures")
	v := cases(x, "positives")[0].(map[string]any)
	m := fieldsAPI(t, "OrderV1", v["fields"].([]any))
	pk := hx(v["public_key_hex"])
	var seed [32]byte
	copy(seed[:], hx(x["test_seed_hex"]))
	_, priv := mldsa65.NewKeyFromSeed(&seed)
	c := Context{ChainID: m["chain_id"].(string), GenesisHash: m["genesis_hash"].(string), ModuleID: m["exchange_module_id"].(string), MarketID: m["market_id"].(string), MarketConfigVersion: m["market_config_version"].(string), RegisteredKey: pk, RegisteredKeyType: "ML-DSA-65", Height: 999, Epoch: number(m, "owner_epoch"), MaxPrice: 1000000, MaxQuantity: 1000000}
	tests := []struct {
		name, want string
		change     func(map[string]any, *Context)
	}{
		{"valid", "OK", func(m map[string]any, c *Context) {}},
		{"equal-expiry", "EXPIRED", func(m map[string]any, c *Context) { c.Height = number(m, "expiry_height") }},
		{"after-expiry", "EXPIRED", func(m map[string]any, c *Context) { c.Height = number(m, "expiry_height") + 1 }},
		{"address", "ADDRESS_MISMATCH", func(m map[string]any, c *Context) { m["owner"] = base64.StdEncoding.EncodeToString(make([]byte, 20)) }},
		{"unregistered", "ACCOUNT_KEY_UNREGISTERED", func(m map[string]any, c *Context) { c.RegisteredKey = nil }},
		{"wrong-key", "ACCOUNT_KEY_MISMATCH", func(m map[string]any, c *Context) { c.RegisteredKey = make([]byte, 1952) }},
		{"wrong-key-type", "ACCOUNT_KEY_MISMATCH", func(m map[string]any, c *Context) { c.RegisteredKeyType = "ed25519" }},
		{"context-before-address", "CONTEXT_MISMATCH", func(m map[string]any, c *Context) {
			c.ChainID = "wrong"
			m["owner"] = base64.StdEncoding.EncodeToString(make([]byte, 20))
		}},
		{"version", "UNSUPPORTED_VERSION", func(m map[string]any, c *Context) { m["protocol_version"] = "2" }},
		{"epoch", "EPOCH_MISMATCH", func(m map[string]any, c *Context) { c.Epoch++ }},
		{"revoked", "ORDER_REVOKED", func(m map[string]any, c *Context) { c.Revoked = true }},
		{"market", "MARKET_LIMIT", func(m map[string]any, c *Context) { m["max_qty_lots"] = "0" }},
		{"fee-cap", "FEE_CAP", func(m map[string]any, c *Context) { c.ActiveFeeBPS = number(m, "max_fee_bps") + 1 }},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			n := map[string]any{}
			for k, v := range m {
				n[k] = v
			}
			ctx := c
			tc.change(n, &ctx)
			b, e := Encode("OrderV1", n)
			if e != nil {
				t.Fatal(e)
			}
			sig := make([]byte, mldsa65.SignatureSize)
			if e = mldsa65.SignTo(priv, Frame("NUS/ORDER/V1", b), nil, false, sig); e != nil {
				t.Fatal(e)
			}
			if got := code(Verify("OrderV1", b, sig, ctx)); got != tc.want {
				t.Fatalf("got %s want %s", got, tc.want)
			}
		})
	}
	b := hx(v["canonical_hex"])
	sig := hx(v["signature_hex"])
	sig[0] ^= 1
	if code(Verify("OrderV1", b, sig, c)) != "INVALID_SIGNATURE" {
		t.Fatal("signature tampering")
	}
}
func TestIntegerBoundaries(t *testing.T) {
	for _, tc := range []struct {
		s     string
		bits  int
		valid bool
	}{{"18446744073709551615", 64, true}, {"18446744073709551616", 64, false}, {"4294967295", 32, true}, {"4294967296", 32, false}, {"0", 64, true}, {"01", 64, false}, {"+1", 64, false}, {"1e3", 64, false}, {"-1", 128, false}} {
		_, e := Integer(tc.s, tc.bits)
		if (e == nil) != tc.valid {
			t.Fatal(tc)
		}
	}
	max := "340282366920938463463374607431768211455"
	if _, e := CheckedArithmetic(max, "1", true); e != nil {
		t.Fatal(e)
	}
	for _, mul := range []bool{true, false} {
		b := "1"
		if mul {
			b = "2"
		}
		if _, e := CheckedArithmetic(max, b, mul); e == nil {
			t.Fatal("overflow")
		}
	}
	if _, e := CheckedArithmetic(strings.Repeat("9", 78), "2", true); e == nil {
		t.Fatal("u256 overflow")
	}
	if f, e := Fee("200", 25); e != nil || f != "1" {
		t.Fatal(f, e)
	}
	if _, e := Fee("1", 25); code(e) != "FEE_GE_RECEIVE" {
		t.Fatal(e)
	}
}
func TestJSONRejections(t *testing.T) {
	v := cases(fixture(t, "message-codec"), "positives")[0].(map[string]any)
	raw, _ := json.Marshal(v["api_json"])
	for _, bad := range [][]byte{append([]byte(`{"amount_atoms":"1",`), raw[1:]...), append(raw, []byte(` {}`)...), bytes.Replace(raw, []byte(`"amount_atoms":"1000000"`), []byte(`"amount_atoms":1000000`), 1), bytes.Replace(raw, []byte(`"genesis_hash":"ab`), []byte(`"genesis_hash":"AB`), 1)} {
		if _, e := EncodeJSON("TransferStableV1", bad); e == nil {
			t.Fatal("accepted malformed JSON")
		}
	}
}
func TestLimits(t *testing.T) {
	if _, e := Decode("OrderV1", make([]byte, 8193)); e != ResourceLimit {
		t.Fatal(e)
	}
	if _, e := Address(make([]byte, 1951)); code(e) != "KEY_LENGTH" {
		t.Fatal(e)
	}
}
func TestSchemaPin(t *testing.T) {
	b, e := os.ReadFile("../../protocol/v1/schema.json")
	if e != nil || !bytes.Equal(b, schemaBytes) {
		t.Fatal("embedded schema differs from pinned contract")
	}
}
func FuzzDecode(f *testing.F) {
	f.Add([]byte{8, 1})
	f.Add([]byte{255, 255, 255})
	f.Fuzz(func(t *testing.T, b []byte) {
		m, e := Decode("OrderV1", b)
		if e == nil {
			out, e := Encode("OrderV1", m)
			if e != nil || !bytes.Equal(out, b) {
				t.Fatal("noncanonical acceptance")
			}
		}
	})
}
func TestSharedIntegers(t *testing.T) {
	b, e := os.ReadFile("../../protocol/v1/vectors/integers.tsv")
	if e != nil {
		t.Fatal(e)
	}
	for _, line := range strings.Split(strings.TrimSuffix(string(b), "\n"), "\n") {
		a := strings.Split(line, "\t")
		t.Run(a[0], func(t *testing.T) {
			got, e := Arithmetic(a[1], a[2], a[3], a[4])
			if e != nil {
				got = e.Error()
			}
			// rc3 supersedes the preserved M0 I20 zero/zero fee expectation.
			if a[0] == "I20" {
				if a[2] != "0" || a[3] != "0" || a[5] != "FEE_GE_RECEIVE" {
					t.Fatal("legacy fixture changed")
				}
				a[5] = "0"
			}
			if got != a[5] {
				t.Fatalf("got %s want %s", got, a[5])
			}
		})
	}
}
func TestS0Boundaries(t *testing.T) {
	for _, v := range cases(fixture(t, "s0-cases"), "cases") {
		v := v.(map[string]any)
		kind := v["kind"].(string)
		if kind != "expiry" && kind != "atoms" && kind != "fill" {
			continue
		}
		t.Run(v["id"].(string), func(t *testing.T) {
			in := v["input"].(map[string]any)
			var result any
			var e error
			switch kind {
			case "expiry":
				h, _ := strconv.ParseUint(in["height"].(string), 10, 64)
				expiry, _ := strconv.ParseUint(in["expiry_height"].(string), 10, 64)
				e = Expiry(h, expiry)
			case "atoms":
				_, e = Atoms(in["value"].(string))
			case "fill":
				result, e = DevFill(in["q"].(string), in["p"].(string), in["bps"].(string))
			}
			if result == nil || e != nil {
				result = code(e)
			}
			a, _ := json.Marshal(result)
			b, _ := json.Marshal(v["expected"])
			if !bytes.Equal(a, b) {
				t.Fatalf("got %s want %s", a, b)
			}
		})
	}
}
func TestBaselineHashes(t *testing.T) {
	b, e := os.ReadFile("../../protocol/v1/manifest.candidate.json")
	if e != nil {
		t.Fatal(e)
	}
	var m struct {
		Contract string            `json:"contract_sha256"`
		Vectors  string            `json:"vectors_sha256"`
		Files    map[string]string `json:"files_sha256"`
	}
	if e = json.Unmarshal(b, &m); e != nil {
		t.Fatal(e)
	}
	if m.Contract != "a71a8c03fea5e4d2876612e821eafcb4b359a0b132157929b9924d6f8fecd73e" || m.Vectors != "4851d9d674b2412ca8919d8347a71da13f9adf4426fe60b44e2a4a259f8bd948" {
		t.Fatal("baseline pin changed")
	}
	for name, want := range m.Files {
		b, e := os.ReadFile("../../protocol/v1/" + name)
		if e != nil {
			t.Fatal(e)
		}
		h := sha256.Sum256(b)
		if hex.EncodeToString(h[:]) != want {
			t.Fatalf("hash mismatch: %s", name)
		}
	}
}
