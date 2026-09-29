package contract

import (
	"encoding/json"
	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
	"os"
	"reflect"
	"testing"
)

func TestRC3Vectors(t *testing.T) {
	b, e := os.ReadFile("../../protocol/v1/vectors/decision-port.json")
	if e != nil {
		t.Fatal(e)
	}
	var v struct {
		Fees []struct {
			ID, Receive string
			Rate        string `json:"active_bps"`
			Expected    string
		} `json:"fee_cases"`
		Caps []struct {
			ID, Cap  string
			Rate     string `json:"active_bps"`
			Expected string
		} `json:"cap_cases"`
		Decisions []struct {
			ID    string
			Input struct {
				Auth     Result    `json:"authentication_result"`
				Snapshot *Snapshot `json:"snapshot"`
			}
			Expected Decision
		} `json:"decision_cases"`
	}
	if e = json.Unmarshal(b, &v); e != nil {
		t.Fatal(e)
	}
	for _, c := range v.Fees {
		t.Run(c.ID, func(t *testing.T) {
			got, e := FeeDecimal(c.Receive, c.Rate)
			if e != nil {
				got = e.Error()
			}
			if got != c.Expected {
				t.Fatalf("%s != %s", got, c.Expected)
			}
		})
	}
	for _, c := range v.Caps {
		t.Run(c.ID, func(t *testing.T) {
			if got := code(CheckCap(c.Cap, c.Rate)); got != c.Expected {
				t.Fatalf("%s != %s", got, c.Expected)
			}
		})
	}
	// These 34 cases test policy/result shape only: auth is explicitly injected.
	for _, c := range v.Decisions {
		t.Run(c.ID, func(t *testing.T) {
			d := Decision{Authentication: c.Input.Auth, SnapshotPolicy: PolicyResult{Result: Result{Status: "NOT_RUN"}, Source: "SYNTHETIC"}, ACK: "NOT_CONNECTED", WALReplay: "NOT_RUN", Ledger: "NOT_CONNECTED"}
			if c.Input.Snapshot != nil {
				d.SnapshotPolicy.SnapshotID = c.Input.Snapshot.ID
			}
			if d.Authentication.Status == "PASS" {
				d.SnapshotPolicy = EvaluateSnapshot(c.Input.Snapshot)
			}
			if !reflect.DeepEqual(d, c.Expected) {
				got, _ := json.Marshal(d)
				want, _ := json.Marshal(c.Expected)
				t.Fatalf("%s != %s", got, want)
			}
		})
	}
}
func TestRC3ActualCryptoDecision(t *testing.T) {
	x := fixture(t, "signatures")
	v := cases(x, "positives")[0].(map[string]any)
	original := fieldsAPI(t, "OrderV1", v["fields"].([]any))
	pk := hx(v["public_key_hex"])
	var seed [32]byte
	copy(seed[:], hx(x["test_seed_hex"]))
	_, priv := mldsa65.NewKeyFromSeed(&seed)
	for _, tc := range []struct {
		name, cap, rate, keyType, auth, policy                string
		binding                                               string
		missingKey, badKey, badSig, missingSnapshot, mismatch bool
	}{
		{name: "zero-fee", cap: "0", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "OK"},
		{name: "small-receive", cap: "25", rate: "25", keyType: "ML-DSA-65", auth: "OK", policy: "FEE_GE_RECEIVE"},
		{name: "cap-10000", cap: "10000", rate: "10000", keyType: "ML-DSA-65", auth: "OK", policy: "FEE_GE_RECEIVE"},
		{name: "cap-10001", cap: "10001", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "OK"},
		{name: "cap-u32-max", cap: "4294967295", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "OK"},
		{name: "active-range", cap: "4294967295", rate: "10001", keyType: "ML-DSA-65", auth: "OK", policy: "BPS_RANGE"},
		{name: "cap-exceeded", cap: "0", rate: "25", keyType: "ML-DSA-65", auth: "OK", policy: "FEE_CAP"},
		{name: "wrong-type", cap: "25", rate: "0", keyType: "OTHER", auth: "ACCOUNT_KEY_MISMATCH"},
		{name: "missing-type", cap: "25", rate: "0", auth: "NOT_CONNECTED"},
		{name: "unregistered", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "ACCOUNT_KEY_UNREGISTERED", missingKey: true},
		{name: "wrong-raw", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "ACCOUNT_KEY_MISMATCH", badKey: true},
		{name: "bad-signature", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "INVALID_SIGNATURE", badSig: true},
		{name: "missing-snapshot", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED", missingSnapshot: true},
		{name: "rc4-binding-epoch_matches", binding: "epoch_matches", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED"},
		{name: "rc4-binding-cap", binding: "cap", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED"},
		{name: "rc4-binding-p", binding: "p", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED"},
		{name: "rc4-binding-expiry_height", binding: "expiry_height", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED"},
		{name: "rc4-binding-height", binding: "height", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED"},
		{name: "rc4-binding-id", binding: "id", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED"},
		{name: "unbound-q", cap: "25", rate: "0", keyType: "ML-DSA-65", auth: "OK", policy: "NOT_CONNECTED", mismatch: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			m := map[string]any{}
			for k, v := range original {
				m[k] = v
			}
			m["max_qty_lots"] = "1"
			m["limit_price_ticks"] = "1"
			m["max_fee_bps"] = tc.cap
			body, e := Encode("OrderV1", m)
			if e != nil {
				t.Fatal(e)
			}
			sig := make([]byte, mldsa65.SignatureSize)
			if e = mldsa65.SignTo(priv, Frame("NUS/ORDER/V1", body), nil, false, sig); e != nil {
				t.Fatal(e)
			}
			if tc.badSig {
				sig[0] ^= 1
			}
			c := Context{SnapshotID: "synthetic-1", ChainID: m["chain_id"].(string), GenesisHash: m["genesis_hash"].(string), ModuleID: m["exchange_module_id"].(string), MarketID: m["market_id"].(string), MarketConfigVersion: m["market_config_version"].(string), RegisteredKey: append([]byte(nil), pk...), RegisteredKeyType: tc.keyType, Height: 999, Epoch: number(m, "owner_epoch")}
			if tc.missingKey {
				c.RegisteredKey = nil
			}
			if tc.badKey {
				c.RegisteredKey[0] ^= 1
			}
			raw := map[string]any{"id": "synthetic-1", "source": "SYNTHETIC", "height": "999", "expiry_height": m["expiry_height"], "epoch_matches": true, "revoked": false, "id_state": "NEW", "cumulative_ok": true, "confirmed_balance_ok": true, "q": "1", "p": "1", "active_bps": tc.rate, "cap": tc.cap}
			if tc.mismatch {
				raw["q"] = "2"
			}
			if tc.binding != "" {
				if tc.binding == "epoch_matches" {
					raw[tc.binding] = false
				} else {
					raw[tc.binding] = "2"
				}
			}
			b, _ := json.Marshal(raw)
			s := new(Snapshot)
			if e = json.Unmarshal(b, s); e != nil {
				t.Fatal(e)
			}
			if tc.missingSnapshot {
				s = nil
			}
			d := DecideOrder(body, sig, c, s)
			if s != nil && !reflect.DeepEqual(d.SnapshotPolicy.SnapshotID, s.ID) {
				t.Fatal("observed ID lost")
			}
			if tc.policy == "NOT_CONNECTED" && d.SnapshotPolicy.Code != nil {
				t.Fatal("unconnected code must be null")
			}
			got := d.Authentication.Status
			if d.Authentication.Code != nil {
				got = *d.Authentication.Code
			}
			if got != tc.auth {
				t.Fatalf("auth %s != %s", got, tc.auth)
			}
			if tc.auth == "OK" {
				got = d.SnapshotPolicy.Status
				if d.SnapshotPolicy.Code != nil {
					got = *d.SnapshotPolicy.Code
				}
				if got != tc.policy {
					t.Fatalf("policy %s != %s", got, tc.policy)
				}
			} else if d.SnapshotPolicy.Status != "NOT_RUN" {
				t.Fatal(d)
			}
			if d.ACK != "NOT_CONNECTED" || d.Ledger != "NOT_CONNECTED" || d.WALReplay != "NOT_RUN" {
				t.Fatal(d)
			}
		})
	}
}

// The 60 rc4 cases inject authentication explicitly; these are specification
// comparisons, not 60 independent ML-DSA verifications.
func TestRC4SnapshotOutputs(t *testing.T) {
	b, err := os.ReadFile("../../protocol/v1/vectors/snapshot-output.json")
	if err != nil {
		t.Fatal(err)
	}
	var v struct {
		Cases []struct {
			ID    string
			Input struct {
				Auth     Result         `json:"authentication_result"`
				Snapshot *Snapshot      `json:"snapshot"`
				Order    map[string]any `json:"authenticated_order"`
				Context  struct {
					ID     *string `json:"snapshot_id"`
					Height *string `json:"height"`
					Epoch  *string `json:"epoch"`
				} `json:"context"`
			}
			Expected Decision
		}
	}
	if err := json.Unmarshal(b, &v); err != nil {
		t.Fatal(err)
	}
	if len(v.Cases) != 60 {
		t.Fatalf("expected 60 rc4 cases, got %d", len(v.Cases))
	}
	for _, tc := range v.Cases {
		t.Run(tc.ID, func(t *testing.T) {
			a := tc.Input
			got := decideAuthenticated(a.Auth, a.Order, a.Context.ID, a.Context.Height, a.Context.Epoch, a.Snapshot)
			if !reflect.DeepEqual(got, tc.Expected) {
				actual, _ := json.Marshal(got)
				want, _ := json.Marshal(tc.Expected)
				t.Fatalf("%s != %s", actual, want)
			}
		})
	}
}
