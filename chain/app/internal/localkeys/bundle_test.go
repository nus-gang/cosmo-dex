//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestInputBundleRustValidation(t *testing.T) {
	binary := os.Getenv("NUS_C_VALIDATOR")
	if binary == "" {
		t.Skip("explicit offline C validator required")
	}
	if !filepath.IsAbs(binary) {
		t.Fatal("absolute binary required")
	}
	for _, fee := range []string{"0", "25"} {
		in := guardFixture(t, fee)
		guard, _, err := PrepareGuard(in, "11111111-2222-3333-4444-555555555555", "fee"+fee)
		if err != nil {
			t.Fatal(err)
		}
		in.Guard = guard
		raw, err := InputBundle(in)
		if err != nil {
			t.Fatal(err)
		}
		var decoded struct {
			Manifest []byte            `json:"runtime_manifest"`
			Files    map[string][]byte `json:"files"`
			Guard    []byte            `json:"guard"`
			Genesis  []byte            `json:"genesis"`
		}
		if json.Unmarshal(raw, &decoded) != nil || !bytes.Equal(decoded.Manifest, in.RuntimeManifest) || !bytes.Equal(decoded.Guard, in.Guard) || !bytes.Equal(decoded.Genesis, in.Genesis) || len(decoded.Files) != len(in.Files) {
			t.Fatal("transport bytes")
		}
		for k, v := range in.Files {
			if !bytes.Equal(v, decoded.Files[k]) {
				t.Fatal("file bytes")
			}
		}
		root := t.TempDir()
		bundle := filepath.Join(root, "bundle.json")
		profile := filepath.Join(root, "profile.json")
		if os.WriteFile(profile, in.EffectiveProfile, 0600) != nil {
			t.Fatal("profile")
		}
		invoke := func(data []byte, accept bool) {
			t.Helper()
			if os.WriteFile(bundle, data, 0600) != nil {
				t.Fatal("bundle")
			}
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, binary, "validate", "--input-set", bundle, "--local-demo-profile", profile, "--runtime-pin", in.ApprovedRuntimeSHA256, "--acknowledge-unproven-space")
			cmd.Env = []string{"PATH=/usr/bin:/bin"}
			out, err := cmd.Output()
			if ctx.Err() != nil {
				t.Fatal("timeout")
			}
			expected := []byte("VALIDATED_INPUT_BYTES_ONLY; RUNTIME_APPROVAL_NOT_ESTABLISHED; durable_ack=false\n")
			if accept && (err != nil || !bytes.Equal(out, expected)) {
				t.Fatalf("C rejected: %v", err)
			}
			if !accept && (err == nil || len(out) != 0) {
				t.Fatal("C accepted mutation")
			}
		}
		invoke(raw, true)
		decoded.Genesis = append(decoded.Genesis, ' ')
		changed, _ := json.Marshal(decoded)
		invoke(changed, false)
		decoded.Genesis = in.Genesis
		decoded.Guard = append(decoded.Guard, ' ')
		changed, _ = json.Marshal(decoded)
		invoke(changed, false)
		invoke(raw, true)
		// Returned bytes are independent; changing caller memory cannot change them.
		in.Genesis = append(in.Genesis, ' ')
		if rejected, err := InputBundle(in); err == nil || rejected != nil {
			t.Fatal("B accepted mutation")
		}
		invoke(raw, true)
	}
}
