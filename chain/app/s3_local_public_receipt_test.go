//go:build dev_local_demo

package app

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"cosmossdk.io/log/v2"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
)

// Synthetic descriptors/pins exercise the public B API, not runtime approval.
func localFullManifest(t *testing.T, in *LocalDemoInputs) {
	t.Helper()
	rebindLocalManifest(t, in, func(m *localManifest) {
		m.Scope = "REVIEWED_RUNTIME"
		for _, name := range []string{"exchange", "settlement", "wallet", "sre"} {
			p := "chain/local-demo/components/" + name + ".json"
			in.Files[p] = localMarshal(t, map[string]any{"head": strings.Repeat("1", 40), "tree": strings.Repeat("2", 40), "implementation_settings": map[string]string{"scope": "SYNTHETIC_OFFLINE_TEST_ONLY"}})
			m.Components[name] = p
			m.Files[p] = localSHA(in.Files[p])
		}
	})
	localGenesisContract(t, in)
}

func localGenesisContract(t *testing.T, in *LocalDemoInputs) {
	t.Helper()
	var m localManifest
	mustTest(t, json.Unmarshal(in.RuntimeManifest, &m))
	changeLocalGenesis(t, in, func(_ *cmttypes.GenesisDoc, g *S3Genesis) { g.ContractHash = m.ContractSHA })
}

func localManifestObject(t *testing.T, in *LocalDemoInputs, change func(map[string]any)) {
	t.Helper()
	var m map[string]any
	mustTest(t, json.Unmarshal(in.RuntimeManifest, &m))
	change(m)
	in.RuntimeManifest = localMarshal(t, m)
	localRepinManifest(t, in)
}

// Test-only re-pinning lets mutations reach checks beyond the outer hash.
func localRepinManifest(t *testing.T, in *LocalDemoInputs) {
	t.Helper()
	in.ApprovedRuntimeSHA256 = localSHA(in.RuntimeManifest)
	changeLocalGuard(t, in, func(g *localGuard) { g.RuntimeSHA = in.ApprovedRuntimeSHA256 })
}

func TestLocalPublicReceiptRejectManifest(t *testing.T) {
	for _, fee := range []int{0, 25} {
		t.Run(fmt.Sprintf("fee%d", fee), func(t *testing.T) {
			base, _ := localFixtureInputs(t, fee)
			localFullManifest(t, &base)
			localEvidence(t, "mutation-base.json", base)
			mutations := map[string]func(*LocalDemoInputs){
				"old_six_fields": func(in *LocalDemoInputs) {
					localManifestObject(t, in, func(m map[string]any) {
						delete(m, "public_receipt_manifest_sha256")
						delete(m, "public_receipt_schema_sha256")
						delete(m, "public_receipt_version")
					})
				},
				"unknown_field": func(in *LocalDemoInputs) {
					localManifestObject(t, in, func(m map[string]any) { m["public_receipt_fallback"] = true })
				},
				"unchanged_approval_pin": func(in *LocalDemoInputs) { in.RuntimeManifest = append(in.RuntimeManifest, ' ') },
				"public_manifest_bytes": func(in *LocalDemoInputs) {
					in.Files[localPublicPath+"MANIFEST.json"] = append(in.Files[localPublicPath+"MANIFEST.json"], ' ')
				},
				"public_schema_bytes": func(in *LocalDemoInputs) { in.Files[localPublicPath+"schema.json"] = []byte("{}") },
				"old_file_set_with_new_pins": func(in *LocalDemoInputs) {
					rebindLocalManifest(t, in, func(m *localManifest) {
						for name := range m.Files {
							if strings.HasPrefix(name, localPublicPath) {
								delete(m.Files, name)
								delete(in.Files, name)
							}
						}
					})
					localGenesisContract(t, in)
				},
				"extra_aggregate_file": func(in *LocalDemoInputs) {
					name := localPublicPath + "extra.json"
					in.Files[name] = []byte("{}")
					rebindLocalManifest(t, in, func(m *localManifest) { m.Files[name] = localSHA(in.Files[name]) })
					localGenesisContract(t, in)
				},
			}
			for _, field := range []string{"public_receipt_manifest_sha256", "public_receipt_schema_sha256", "public_receipt_version"} {
				for _, kind := range []string{"missing", "null", "wrong", "case", "duplicate"} {
					mutations[field+"_"+kind] = func(in *LocalDemoInputs) {
						if kind == "duplicate" {
							in.RuntimeManifest = append([]byte(fmt.Sprintf(`{"%s":"wrong",`, field)), in.RuntimeManifest[1:]...)
							localRepinManifest(t, in)
							return
						}
						localManifestObject(t, in, func(m map[string]any) {
							switch kind {
							case "missing":
								delete(m, field)
							case "null":
								m[field] = nil
							case "wrong":
								m[field] = "wrong"
							case "case":
								m[strings.ToUpper(field)] = m[field]
								delete(m, field)
							}
						})
					}
				}
			}
			for _, name := range []string{"MANIFEST.json", "schema.json", "vectors/fee0-taker-trade.frame"} {
				for _, kind := range []string{"missing_resealed", "changed_resealed"} {
					mutations[name+"_"+kind] = func(in *LocalDemoInputs) {
						p := localPublicPath + name
						rebindLocalManifest(t, in, func(m *localManifest) {
							if kind == "missing_resealed" {
								delete(in.Files, p)
								delete(m.Files, p)
							} else {
								in.Files[p] = append(in.Files[p], ' ')
								m.Files[p] = localSHA(in.Files[p])
							}
						})
						localGenesisContract(t, in)
					}
				}
			}
			for name, change := range mutations {
				t.Run(name, func(t *testing.T) {
					in := copyLocalInputs(t, base)
					change(&in)
					before := localMarshal(t, in)
					ctx, err := ValidateLocalDemo(in)
					if err == nil || ctx != nil || !bytes.Equal(before, localMarshal(t, in)) {
						t.Fatalf("invalid input accepted or mutated: %v", err)
					}
					// Preserve exact mutations against one shared raw input, rather
					// than duplicating hundreds of unchanged source files per case.
					changed := map[string][]byte{}
					removed := []string{}
					for p, raw := range in.Files {
						if !bytes.Equal(raw, base.Files[p]) {
							changed[p] = raw
						}
					}
					for p := range base.Files {
						if _, ok := in.Files[p]; !ok {
							removed = append(removed, p)
						}
					}
					localEvidence(t, "rejection.json", map[string]any{"case": name, "error": err.Error(), "result": "PASS", "input_sha256": localSHA(before), "runtime_manifest": in.RuntimeManifest, "approved_runtime_sha256": in.ApprovedRuntimeSHA256, "genesis": in.Genesis, "guard": in.Guard, "changed_files": changed, "removed_files": removed, "input_mutation": false})
				})
			}
		})
	}
}

func TestLocalPublicReceiptRuntimeInitQueryRestart(t *testing.T) {
	for _, fee := range []int{0, 25} {
		t.Run(fmt.Sprintf("fee%d", fee), func(t *testing.T) {
			in, keys := localFixtureInputs(t, fee)
			localFullManifest(t, &in)
			ctx, err := ValidateLocalDemo(in)
			mustTest(t, err)
			var m localManifest
			mustTest(t, json.Unmarshal(in.RuntimeManifest, &m))
			oldFiles := map[string]string{}
			for name, digest := range m.Files {
				if !strings.HasPrefix(name, localPublicPath) {
					oldFiles[name] = digest
				}
			}
			oldContract := localAggregate(oldFiles)
			if ctx["contract_hash"] != m.ContractSHA || oldContract == m.ContractSHA || len(m.Files)-len(oldFiles) != 61 {
				t.Fatal("public source aggregate not bound")
			}
			localEvidence(t, "public-inputs.json", in)
			f := &s3Fixture{fixture: &fixture{keys: keys}, users: 2, operator: 2, genesis: in.Genesis, dir: t.TempDir()}
			hash := sha256.Sum256(in.Genesis)
			f.hash = hash[:]
			f.db, err = dbm.NewDB("application", dbm.GoLevelDBBackend, f.dir)
			mustTest(t, err)
			t.Cleanup(func() { _ = f.db.Close() })
			f.a, err = NewLocalDemo(f.db, log.NewNopLogger(), in)
			mustTest(t, err)
			old := copyLocalInputs(t, in)
			changeLocalGenesis(t, &old, func(_ *cmttypes.GenesisDoc, g *S3Genesis) { g.ContractHash = oldContract })
			_, err = f.a.InitChain(localInitRequest(t, old))
			if err == nil || !strings.Contains(err.Error(), "INIT_GENESIS_MISMATCH") {
				t.Fatalf("old contract InitChain accepted: %v", err)
			}
			_, err = f.a.InitChain(localInitRequest(t, in))
			mustTest(t, err)
			f.blocks(t)
			if !reflect.DeepEqual(f.a.Exchange.Context(f.ctx(t)), ctx) {
				t.Fatal("initialized context mismatch")
			}
			f.deposit(t, 0, ex.Base, "10000000")
			f.deposit(t, 1, ex.Quote, "100000000")
			wire := f.demo(t)
			requireCode(t, f.submit(t, wire), "")
			before := f.exchangeState(t)
			requireCode(t, f.submit(t, wire), "")
			if !reflect.DeepEqual(before, f.exchangeState(t)) {
				t.Fatal("duplicate settlement changed state")
			}
			query := localQuery(t, f, ctx, "Batch", "batch_seq", "1")
			if query.Code != 0 {
				t.Fatal(query.Log)
			}
			oldContext := map[string]string{}
			for name, value := range ctx {
				oldContext[name] = value
			}
			oldContext["contract_hash"] = oldContract
			badQuery := localQuery(t, f, oldContext, "Batch", "batch_seq", "1")
			if badQuery.Code == 0 || !strings.Contains(badQuery.Log, "CONTEXT_MISMATCH") {
				t.Fatal("old contract query accepted")
			}
			localEvidence(t, "old-contract-query.json", badQuery)
			// A fully self-consistent, newly pinned input set still cannot adopt
			// this database or reissue its guard. Validation alone is not migration.
			other := copyLocalInputs(t, in)
			p := m.Components["chain"]
			other.Files[p] = bytes.Replace(other.Files[p], []byte("COMPONENT_FIXTURE"), []byte("DIFFERENT_FIXTURE"), 1)
			rebindLocalManifest(t, &other, func(m *localManifest) { m.Files[p] = localSHA(other.Files[p]) })
			localGenesisContract(t, &other)
			_, err = ValidateLocalDemo(other)
			mustTest(t, err)
			for n := 1; n <= 2; n++ {
				mustTest(t, f.db.Close())
				f.db, err = dbm.NewDB("application", dbm.GoLevelDBBackend, f.dir)
				mustTest(t, err)
				if _, err = NewLocalDemo(f.db, log.NewNopLogger(), other); err == nil || !strings.Contains(err.Error(), "genesis hash differs") {
					t.Fatalf("different contract/genesis reopened database: %v", err)
				}
				localEvidence(t, fmt.Sprintf("restart-%d-rejected.json", n), map[string]any{"error": err.Error(), "other": other})
				f.a, err = NewLocalDemo(f.db, log.NewNopLogger(), in)
				mustTest(t, err)
				after := localQuery(t, f, ctx, "Batch", "batch_seq", "1")
				if after.Code != 0 || !bytes.Equal(query.Value, after.Value) || !reflect.DeepEqual(before, f.exchangeState(t)) {
					t.Fatal("restart receipt/state changed")
				}
				localEvidence(t, fmt.Sprintf("restart-%d.json", n), map[string]any{"before": query, "after": after})
			}
			r, err := f.a.Exchange.StoredReceipt(f.ctx(t), 1, f.a.Exchange.Last(f.ctx(t)))
			mustTest(t, err)
			if !reflect.DeepEqual(r.Context, ctx) {
				t.Fatal("receipt lost public contract Context")
			}
			localEvidence(t, "result.json", map[string]any{"result": "PASS", "scope": "SYNTHETIC_PUBLIC_API_SDK_ONLY", "DEV01_14": "NOT_RUN", "AR01_14": "NOT_RUN", "old_contract_hash": oldContract, "context": ctx, "receipt": r, "expected_state_diff": map[string]any{}})
		})
	}
}
