package main

import (
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/nus-gang/cosmo-dex/chain/contract"
)

// Exercise the real stdin/stdout process, not only the typed decision boundary.
func TestJSONLContextPresence(t *testing.T) {
	raw, err := os.ReadFile("../../../protocol/v1/vectors/signatures.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixtures struct {
		Positives []struct {
			Wire      string `json:"canonical_hex"`
			Signature string `json:"signature_hex"`
			Key       string `json:"public_key_hex"`
		} `json:"positives"`
	}
	if err = json.Unmarshal(raw, &fixtures); err != nil {
		t.Fatal(err)
	}
	v := fixtures.Positives[0]
	wire, err := hex.DecodeString(v.Wire)
	if err != nil {
		t.Fatal(err)
	}
	m, err := contract.Decode("OrderV1", wire)
	if err != nil {
		t.Fatal(err)
	}
	key, err := hex.DecodeString(v.Key)
	if err != nil {
		t.Fatal(err)
	}
	bin := filepath.Join(t.TempDir(), "runner")
	if output, err := exec.Command("go", "build", "-mod=readonly", "-o", bin, ".").CombinedOutput(); err != nil {
		t.Fatalf("build: %v %s", err, output)
	}
	for _, tc := range []struct {
		name, field, mode, status, code string
		epochMatches                    bool
	}{
		{"explicit-height-zero", "", "", "PASS", "OK", true},
		{"explicit-epoch-zero", "Epoch", "zero", "REJECTED", "EPOCH_MISMATCH", false},
		{"height-missing", "Height", "missing", "NOT_CONNECTED", "", true},
		{"height-null", "Height", "null", "NOT_CONNECTED", "", true},
		{"epoch-missing", "Epoch", "missing", "NOT_CONNECTED", "", false},
		{"epoch-null", "Epoch", "null", "NOT_CONNECTED", "", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var epoch uint64
			if err := json.Unmarshal([]byte(m["owner_epoch"].(string)), &epoch); err != nil {
				t.Fatal(err)
			}
			if epoch == 0 {
				t.Fatal("fixture requires nonzero epoch")
			}
			context := map[string]any{"SnapshotID": "jsonl-presence", "ChainID": m["chain_id"], "GenesisHash": m["genesis_hash"], "ModuleID": m["exchange_module_id"], "MarketID": m["market_id"], "MarketConfigVersion": m["market_config_version"], "RegisteredKey": base64.StdEncoding.EncodeToString(key), "RegisteredKeyType": "ML-DSA-65", "Height": 0, "Epoch": epoch}
			switch tc.mode {
			case "missing":
				delete(context, tc.field)
			case "null":
				context[tc.field] = nil
			case "zero":
				context[tc.field] = 0
			}
			snapshot := map[string]any{"id": "jsonl-presence", "source": "SYNTHETIC", "height": "0", "expiry_height": m["expiry_height"], "epoch_matches": tc.epochMatches, "revoked": false, "id_state": "NEW", "cumulative_ok": true, "confirmed_balance_ok": true, "q": m["max_qty_lots"], "p": m["limit_price_ticks"], "active_bps": "0", "cap": m["max_fee_bps"]}
			input, err := json.Marshal(map[string]any{"op": "decide_order", "wire_hex": v.Wire, "signature_hex": v.Signature, "context": context, "snapshot": snapshot})
			if err != nil {
				t.Fatal(err)
			}
			cmd := exec.Command(bin)
			cmd.Stdin = bytes.NewReader(append(input, '\n'))
			output, err := cmd.Output()
			if err != nil {
				t.Fatal(err)
			}
			var got contract.Decision
			if err = json.Unmarshal(output, &got); err != nil {
				t.Fatal(err)
			}
			t.Logf("output=%s", output)
			if got.Authentication.Status != "PASS" || got.Authentication.Code == nil || *got.Authentication.Code != "OK" {
				t.Fatalf("authentication: %s", output)
			}
			p := got.SnapshotPolicy
			if p.Status != tc.status || (tc.code == "" && p.Code != nil) || (tc.code != "" && (p.Code == nil || *p.Code != tc.code)) {
				t.Fatalf("policy: %s", output)
			}
			if p.SnapshotID == nil || *p.SnapshotID != "jsonl-presence" || p.Source != "SYNTHETIC" || got.ACK != "NOT_CONNECTED" || got.Ledger != "NOT_CONNECTED" || got.WALReplay != "NOT_RUN" {
				t.Fatalf("boundaries: %s", output)
			}
		})
	}
}
