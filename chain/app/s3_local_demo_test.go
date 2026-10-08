//go:build dev_local_demo

package app

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"

	"cosmossdk.io/log/v2"
	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/crypto/ed25519"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

// The fixture entry point exists only in this test file.
func newLocalDemoComponent(db dbm.DB, logger log.Logger, in LocalDemoInputs) (*App, error) {
	return newLocalDemo(db, logger, in, true)
}

func localMarshal(t *testing.T, v any) []byte {
	t.Helper()
	raw, err := canonicalJSON(v)
	mustTest(t, err)
	return raw
}
func localRead(t *testing.T, name string) []byte {
	t.Helper()
	raw, err := os.ReadFile(name)
	mustTest(t, err)
	return raw
}
func copyLocalInputs(t *testing.T, in LocalDemoInputs) LocalDemoInputs {
	var out LocalDemoInputs
	mustTest(t, json.Unmarshal(localMarshal(t, in), &out))
	return out
}
func changeLocalGuard(t *testing.T, in *LocalDemoInputs, change func(*localGuard)) {
	var g localGuard
	mustTest(t, json.Unmarshal(in.Guard, &g))
	change(&g)
	in.Guard = localMarshal(t, g)
}
func rebindLocalManifest(t *testing.T, in *LocalDemoInputs, change func(*localManifest)) {
	var m localManifest
	mustTest(t, json.Unmarshal(in.RuntimeManifest, &m))
	change(&m)
	m.ContractSHA = localAggregate(m.Files)
	in.RuntimeManifest = localMarshal(t, m)
	in.ApprovedRuntimeSHA256 = localSHA(in.RuntimeManifest)
	changeLocalGuard(t, in, func(g *localGuard) {
		g.RuntimeSHA = in.ApprovedRuntimeSHA256
		g.Context["contract_hash"] = m.ContractSHA
	})
}
func changeLocalGenesis(t *testing.T, in *LocalDemoInputs, change func(*cmttypes.GenesisDoc, *S3Genesis)) {
	doc, err := cmttypes.GenesisDocFromJSON(in.Genesis)
	mustTest(t, err)
	var g S3Genesis
	mustTest(t, json.Unmarshal(doc.AppState, &g))
	change(doc, &g)
	doc.AppState = localMarshal(t, g)
	in.Genesis, err = cmtjson.Marshal(doc)
	mustTest(t, err)
	changeLocalGuard(t, in, func(g *localGuard) { g.Context["genesis_hash"] = localSHA(in.Genesis) })
}
func localFixtureInputs(t *testing.T, fee int) (LocalDemoInputs, []mldsa65.PrivKey) {
	t.Helper()
	in := LocalDemoInputs{Files: map[string][]byte{}, AcknowledgeUnprovenSpace: true}
	baselineRaw := localRead(t, "../../protocol/s3/manifest.json")
	var baseline struct {
		Files map[string]string `json:"files_sha256"`
	}
	mustTest(t, json.Unmarshal(baselineRaw, &baseline))
	for name := range baseline.Files {
		file := filepath.Join("../..", name)
		if name == "chain/app/go.mod" {
			file = "testdata/local-demo/rc3-chain-app-go.mod"
		}
		in.Files[name] = localRead(t, file)
	}
	in.Files["protocol/s3/manifest.json"] = baselineRaw
	var candidate struct {
		Files map[string]string `json:"files_sha256"`
	}
	raw := localRead(t, "../../"+localCandidatePath+"MANIFEST.json")
	mustTest(t, json.Unmarshal(raw, &candidate))
	in.Files[localCandidatePath+"MANIFEST.json"] = raw
	for name := range candidate.Files {
		in.Files[localCandidatePath+name] = localRead(t, "../../"+localCandidatePath+name)
	}
	var public struct {
		Files map[string]string `json:"files_sha256"`
	}
	raw = localRead(t, "../../"+localPublicPath+"MANIFEST.json")
	mustTest(t, json.Unmarshal(raw, &public))
	in.Files[localPublicPath+"MANIFEST.json"] = raw
	for name := range public.Files {
		in.Files[name] = localRead(t, "../../"+name)
	}
	descriptor := "chain/local-demo/components/chain.json"
	// Historical base pins plus exact patched sources identify an OFFLINE fixture,
	// not a reviewed new component or final integrated runtime.
	settings := map[string]string{"scope": "COMPONENT_FIXTURE", "chain_app_go_mod": localSHA(localRead(t, "go.mod"))}
	for _, name := range []string{"app.go", "s3.go", "s3_local_demo.go", "x/exchange/keeper/keeper.go", "x/exchange/keeper/s3_store.go"} {
		settings[name] = localSHA(localRead(t, name))
	}
	in.Files[descriptor] = localMarshal(t, map[string]any{"head": "f44d511bee7ced4f95dce029ddd7abcf7d8e6722", "tree": "ca10dfdd5ece500af13f866f520c7da8d5a91f65", "implementation_settings": settings})
	m := localManifest{Format: "s3-dev-local-runtime/1", Scope: "COMPONENT_FIXTURE", CandidateSHA: localCandidateSHA, PublicManifestSHA: localPublicManifestSHA, PublicSchemaSHA: localPublicSchemaSHA, PublicVersion: localPublicVersion, Files: map[string]string{}, Components: map[string]string{"chain": descriptor}}
	for name, raw := range in.Files {
		m.Files[name] = localSHA(raw)
	}
	m.ContractSHA = localAggregate(m.Files)
	in.RuntimeManifest = localMarshal(t, m)
	in.ApprovedRuntimeSHA256 = localSHA(in.RuntimeManifest)
	in.EffectiveProfile = in.Files[fmt.Sprintf("%seffective-profile-fee%d.json", localCandidatePath, fee)]
	keys := []mldsa65.PrivKey{}
	g := S3Genesis{FeeBPS: strconv.Itoa(fee), ContractHash: m.ContractSHA, ConfigHash: localSHA(in.EffectiveProfile)}
	for i := 0; i < 5; i++ {
		// Public synthetic component keys only; never used by a node/service.
		seed := sha256.Sum256([]byte(fmt.Sprintf("nus55-local-component-fee%d-key%d", fee, i)))
		key, err := mldsa65.GenPrivKeyFromSeed(seed[:])
		mustTest(t, err)
		keys = append(keys, key)
		switch {
		case i < 2:
			g.PublicKeys = append(g.PublicKeys, key.PubKey().Bytes())
		case i < 4:
			g.OperatorKeys = append(g.OperatorKeys, key.PubKey().Bytes())
		default:
			g.AdminKey = key.PubKey().Bytes()
		}
	}
	params := cmttypes.DefaultConsensusParams()
	params.Block.MaxBytes = 1048576
	params.Block.MaxGas = 20000000
	params.Evidence.MaxBytes = 65536
	validators := []cmttypes.GenesisValidator{}
	for i := 0; i < 4; i++ {
		pk := ed25519.GenPrivKeyFromSecret([]byte(fmt.Sprintf("local-component-fee%d-validator%d", fee, i))).PubKey()
		validators = append(validators, cmttypes.GenesisValidator{Address: pk.Address(), PubKey: pk, Power: 10, Name: fmt.Sprintf("v%d", i)})
	}
	doc := &cmttypes.GenesisDoc{GenesisTime: time.Unix(1700000000, 0).UTC(), ChainID: ex.S3ChainID, InitialHeight: 1, ConsensusParams: params, Validators: validators, AppState: localMarshal(t, g)}
	var err error
	in.Genesis, err = cmtjson.Marshal(doc)
	mustTest(t, err)
	guard := localGuard{Envelope: "s3-dev-local/1", Profile: localProfileID, CandidateSHA: localCandidateSHA, RuntimeSHA: in.ApprovedRuntimeSHA256, EffectiveSHA: localSHA(in.EffectiveProfile), RunUUID: fmt.Sprintf("00000000-0000-4000-8000-%012d", fee+1), FeeProfile: fmt.Sprintf("fee%d", fee), Context: map[string]string{"service_schema": "s3/3", "chain_id": ex.S3ChainID, "genesis_hash": localSHA(in.Genesis), "contract_hash": m.ContractSHA, "config_hash": localSHA(in.EffectiveProfile), "market_id": ex.S3Market, "market_config_version": "1"}}
	in.Guard = localMarshal(t, guard)
	return in, keys
}
func localInitRequest(t *testing.T, in LocalDemoInputs) *abci.RequestInitChain {
	doc, err := cmttypes.GenesisDocFromJSON(in.Genesis)
	mustTest(t, err)
	params := doc.ConsensusParams.ToProto()
	updates := []abci.ValidatorUpdate{}
	for _, v := range doc.Validators {
		updates = append(updates, abci.Ed25519ValidatorUpdate(v.PubKey.Bytes(), v.Power))
	}
	return &abci.RequestInitChain{Time: doc.GenesisTime, ChainId: doc.ChainID, InitialHeight: doc.InitialHeight, ConsensusParams: &params, Validators: updates, AppStateBytes: doc.AppState}
}
func newLocalFixture(t *testing.T, fee int) (*s3Fixture, LocalDemoInputs) {
	t.Helper()
	in, keys := localFixtureInputs(t, fee)
	f := &s3Fixture{fixture: &fixture{keys: keys}, users: 2, operator: 2, genesis: in.Genesis, dir: t.TempDir()}
	hash := sha256.Sum256(in.Genesis)
	f.hash = hash[:]
	var err error
	f.db, err = dbm.NewDB("application", dbm.GoLevelDBBackend, f.dir)
	mustTest(t, err)
	f.a, err = newLocalDemoComponent(f.db, log.NewNopLogger(), in)
	mustTest(t, err)
	_, err = f.a.InitChain(localInitRequest(t, in))
	mustTest(t, err)
	f.blocks(t)
	t.Cleanup(func() { _ = f.db.Close() })
	return f, in
}
func localEvidence(t *testing.T, name string, v any) {
	if root := os.Getenv("NUS_S3_EVIDENCE_DIR"); root != "" {
		dir := filepath.Join(root, strings.ReplaceAll(t.Name(), "/", "__"))
		mustTest(t, os.MkdirAll(dir, 0700))
		mustTest(t, os.WriteFile(filepath.Join(dir, name), localMarshal(t, v), 0600))
	}
}
func localQuery(t *testing.T, f *s3Fixture, ctx map[string]string, method, extra, value string) *abci.ResponseQuery {
	t.Helper()
	in := map[string]any{"context": ctx, "height": strconv.FormatInt(f.h, 10)}
	if extra != "" {
		in[extra] = value
	}
	raw := localMarshal(t, in)
	out, err := f.a.Query(context.Background(), &abci.RequestQuery{Path: "/nus.exchange.s3.v1.Query/" + method, Height: f.h, Data: raw})
	mustTest(t, err)
	localEvidence(t, "query-"+method+".json", map[string]any{"input": raw, "output": out})
	return out
}

func TestLocalDemoInitQueryRestart(t *testing.T) {
	for _, fee := range []int{0, 25} {
		t.Run(fmt.Sprintf("fee%d", fee), func(t *testing.T) {
			f, in := newLocalFixture(t, fee)
			localEvidence(t, "component-inputs.json", in)
			var guard localGuard
			mustTest(t, json.Unmarshal(in.Guard, &guard))
			if !reflect.DeepEqual(f.a.Exchange.Context(f.ctx(t)), guard.Context) {
				t.Fatal("init context mismatch")
			}
			if out := localQuery(t, f, guard.Context, "Snapshot", "", ""); out.Code != 0 {
				t.Fatal(out.Log)
			}
			f.deposit(t, 0, ex.Base, "10000000")
			f.deposit(t, 1, ex.Quote, "100000000")
			wire := f.demo(t)
			requireCode(t, f.submit(t, wire), "")
			base, quote := "1000000", "10000000"
			if fee == 25 {
				base, quote = "997500", "9975000"
			}
			f.balance(t, 1, ex.Base, base)
			f.balance(t, 0, ex.Quote, quote)
			before := f.exchangeState(t)
			requireCode(t, f.submit(t, wire), "")
			if !reflect.DeepEqual(before, f.exchangeState(t)) {
				t.Fatal("retry changed asset state")
			}
			receipt, err := f.a.Exchange.StoredReceipt(f.ctx(t), 1, f.a.Exchange.Last(f.ctx(t)))
			mustTest(t, err)
			if !reflect.DeepEqual(receipt.Context, guard.Context) {
				t.Fatal("receipt context mismatch")
			}
			query := localQuery(t, f, guard.Context, "Batch", "batch_seq", "1")
			if query.Code != 0 {
				t.Fatal(query.Log)
			}
			mustTest(t, f.db.Close())
			f.db, err = dbm.NewDB("application", dbm.GoLevelDBBackend, f.dir)
			mustTest(t, err)
			if _, err = NewForChain(f.db, f.hash, log.NewNopLogger(), ex.S3ChainID); err == nil {
				t.Fatal("standard reopened development DB")
			}
			changed := copyLocalInputs(t, in)
			changeLocalGuard(t, &changed, func(g *localGuard) { g.RunUUID = "11111111-1111-4111-8111-111111111111" })
			if _, err = newLocalDemoComponent(f.db, log.NewNopLogger(), changed); err == nil || !strings.Contains(err.Error(), "S3_BINDING_MISMATCH") {
				t.Fatalf("restart guard: %v", err)
			}
			f.a, err = newLocalDemoComponent(f.db, log.NewNopLogger(), in)
			mustTest(t, err)
			after := localQuery(t, f, guard.Context, "Batch", "batch_seq", "1")
			if after.Code != 0 || !bytes.Equal(query.Value, after.Value) {
				t.Fatal("same-height restart query changed")
			}
			localEvidence(t, "restart-queries.json", map[string]any{"before": query, "after": after, "height": f.h})
			original, err := f.a.Exchange.StoredReceipt(f.ctx(t), 1, f.a.Exchange.Last(f.ctx(t)))
			mustTest(t, err)
			if !reflect.DeepEqual(receipt, original) || !reflect.DeepEqual(before, f.exchangeState(t)) {
				t.Fatal("restart receipt/state changed")
			}
			localEvidence(t, "result.json", map[string]any{"result": "PASS", "scope": "SDK_COMPONENT_FIXTURE", "DEV03": "NOT_RUN", "fee_bps": strconv.Itoa(fee), "context": guard.Context, "height": f.h, "batch_wire": wire, "receipt": original, "expected_state_diff": map[string]any{}})
		})
	}
}

func TestLocalDemoRejectInputBinding(t *testing.T) {
	for _, fee := range []int{0, 25} {
		t.Run(fmt.Sprintf("fee%d", fee), func(t *testing.T) {
			base, _ := localFixtureInputs(t, fee)
			mutations := map[string]func(*LocalDemoInputs){
				"missing_ack":     func(i *LocalDemoInputs) { i.AcknowledgeUnprovenSpace = false },
				"missing_profile": func(i *LocalDemoInputs) { i.EffectiveProfile = nil },
				"runtime_pin":     func(i *LocalDemoInputs) { i.ApprovedRuntimeSHA256 = strings.Repeat("0", 64) },
				"candidate_as_runtime": func(i *LocalDemoInputs) {
					i.RuntimeManifest = i.Files[localCandidatePath+"MANIFEST.json"]
					i.ApprovedRuntimeSHA256 = localSHA(i.RuntimeManifest)
				},
				"file_bytes":   func(i *LocalDemoInputs) { i.Files["protocol/s3/CONTRACT.md"] = []byte("changed") },
				"missing_file": func(i *LocalDemoInputs) { delete(i.Files, "protocol/s3/CONTRACT.md") },
				"extra_file":   func(i *LocalDemoInputs) { i.Files["genesis.json"] = i.Genesis },
				"resealed_inherited_file": func(i *LocalDemoInputs) {
					i.Files["protocol/s3/CONTRACT.md"] = []byte("changed")
					rebindLocalManifest(t, i, func(m *localManifest) {
						m.Files["protocol/s3/CONTRACT.md"] = localSHA(i.Files["protocol/s3/CONTRACT.md"])
					})
				},
				"candidate_pin": func(i *LocalDemoInputs) {
					changeLocalGuard(t, i, func(g *localGuard) { g.CandidateSHA = strings.Repeat("0", 64) })
				},
				"guard_runtime_pin": func(i *LocalDemoInputs) {
					changeLocalGuard(t, i, func(g *localGuard) { g.RuntimeSHA = strings.Repeat("0", 64) })
				},
				"guard_profile_hash": func(i *LocalDemoInputs) {
					changeLocalGuard(t, i, func(g *localGuard) { g.EffectiveSHA = strings.Repeat("0", 64) })
				},
				"guard_other_fee": func(i *LocalDemoInputs) {
					changeLocalGuard(t, i, func(g *localGuard) {
						if fee == 0 {
							g.FeeProfile = "fee25"
						} else {
							g.FeeProfile = "fee0"
						}
					})
				},
				"guard_extra":     func(i *LocalDemoInputs) { i.Guard = append([]byte(`{"extra":false,`), i.Guard[1:]...) },
				"guard_duplicate": func(i *LocalDemoInputs) { i.Guard = append([]byte(`{"profile_id":"s3-dev-local-v1",`), i.Guard[1:]...) },
				"guard_missing": func(i *LocalDemoInputs) {
					var g map[string]any
					mustTest(t, json.Unmarshal(i.Guard, &g))
					delete(g, "profile_id")
					i.Guard = localMarshal(t, g)
				},
				"guard_wrong_case": func(i *LocalDemoInputs) {
					i.Guard = bytes.Replace(i.Guard, []byte(`"profile_id"`), []byte(`"PROFILE_ID"`), 1)
				},
				"guard_noncanonical": func(i *LocalDemoInputs) { i.Guard = append(i.Guard, '\n') },
				"old_context": func(i *LocalDemoInputs) {
					changeLocalGuard(t, i, func(g *localGuard) { g.Context["service_schema"] = "s3/1" })
				},
				"unknown_profile":     func(i *LocalDemoInputs) { changeLocalGuard(t, i, func(g *localGuard) { g.Profile = "s3-standard" }) },
				"genesis_exact_bytes": func(i *LocalDemoInputs) { i.Genesis = append(i.Genesis, '\n') },
				"genesis_contract": func(i *LocalDemoInputs) {
					changeLocalGenesis(t, i, func(_ *cmttypes.GenesisDoc, g *S3Genesis) { g.ContractHash = ex.S3ContractHash })
				},
				"genesis_config": func(i *LocalDemoInputs) {
					changeLocalGenesis(t, i, func(_ *cmttypes.GenesisDoc, g *S3Genesis) { g.ConfigHash = ex.S3ConfigHash })
				},
				"genesis_other_fee": func(i *LocalDemoInputs) {
					changeLocalGenesis(t, i, func(_ *cmttypes.GenesisDoc, g *S3Genesis) {
						if fee == 0 {
							g.FeeBPS = "25"
						} else {
							g.FeeBPS = "0"
						}
					})
				},
				"genesis_consensus": func(i *LocalDemoInputs) {
					changeLocalGenesis(t, i, func(d *cmttypes.GenesisDoc, _ *S3Genesis) { d.ConsensusParams.Block.MaxGas++ })
				},
			}
			for _, field := range []string{"chain_id", "genesis_hash", "contract_hash", "config_hash", "market_id", "market_config_version"} {
				field := field
				mutations["context_"+field] = func(i *LocalDemoInputs) { changeLocalGuard(t, i, func(g *localGuard) { g.Context[field] = "wrong" }) }
			}
			for name, mutation := range mutations {
				t.Run(name, func(t *testing.T) {
					in := copyLocalInputs(t, base)
					mutation(&in)
					binding, _, err := validateLocalDemo(in, true)
					if err == nil || binding != nil {
						t.Fatal("negative input accepted")
					}
					localEvidence(t, "rejection.json", map[string]any{"case": name, "error": err.Error(), "result": "PASS"})
				})
			}
			if _, err := ValidateLocalDemo(base); err == nil {
				t.Fatal("component fixture accepted as final runtime")
			}
		})
	}
}

func TestLocalDemoInitRejectsDifferentRequest(t *testing.T) {
	in, _ := localFixtureInputs(t, 0)
	cases := map[string]func(*abci.RequestInitChain){
		"state":     func(r *abci.RequestInitChain) { r.AppStateBytes = append(r.AppStateBytes, ' ') },
		"time":      func(r *abci.RequestInitChain) { r.Time = r.Time.Add(time.Second) },
		"chain":     func(r *abci.RequestInitChain) { r.ChainId = ex.S2ChainID },
		"height":    func(r *abci.RequestInitChain) { r.InitialHeight = 2 },
		"consensus": func(r *abci.RequestInitChain) { r.ConsensusParams.Block.MaxGas++ },
		"validator": func(r *abci.RequestInitChain) { r.Validators[0].Power++ },
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			db := dbm.NewMemDB()
			defer db.Close()
			a, err := newLocalDemoComponent(db, log.NewNopLogger(), in)
			mustTest(t, err)
			req := localInitRequest(t, in)
			change(req)
			if _, err = a.InitChain(req); err == nil {
				t.Fatal("different InitChain accepted")
			}
			// Rejection occurred before SDK cache/consensus mutation.
			valid := localInitRequest(t, in)
			valid.Validators[0], valid.Validators[3] = valid.Validators[3], valid.Validators[0]
			_, err = a.InitChain(valid)
			mustTest(t, err)
			if a.LastBlockHeight() != 0 {
				t.Fatal("rejected init committed state")
			}
		})
	}
}

func TestLocalDemoQueryAndSignatureBinding(t *testing.T) {
	for _, fee := range []int{0, 25} {
		t.Run(fmt.Sprintf("fee%d", fee), func(t *testing.T) {
			f, in := newLocalFixture(t, fee)
			f.deposit(t, 0, ex.Base, "10000000")
			f.deposit(t, 1, ex.Quote, "100000000")
			var g localGuard
			mustTest(t, json.Unmarshal(in.Guard, &g))
			for _, field := range []string{"service_schema", "chain_id", "genesis_hash", "contract_hash", "config_hash", "market_id", "market_config_version"} {
				t.Run("query_"+field, func(t *testing.T) {
					wrong := map[string]string{}
					for k, v := range g.Context {
						wrong[k] = v
					}
					wrong[field] = "wrong"
					if field == "service_schema" {
						wrong[field] = "s3/1"
					}
					for _, method := range []string{"Snapshot", "Batch", "Order"} {
						extra, value := "", ""
						if method == "Batch" {
							extra, value = "batch_seq", "1"
						}
						if method == "Order" {
							extra, value = "order_hash", strings.Repeat("0", 64)
						}
						out := localQuery(t, f, wrong, method, extra, value)
						if out.Code == 0 || !strings.Contains(out.Log, "CONTEXT_MISMATCH") {
							t.Fatalf("%s accepted wrong %s", method, field)
						}
					}
				})
			}
			for _, kind := range []string{"signed_wrong_genesis", "signed_other_fee_genesis", "signature_tamper", "other_fee", "old_batch_genesis"} {
				t.Run(kind, func(t *testing.T) {
					sell := f.proof(t, 0, 1, 2, 2000, 10000, 100)
					buy := f.proof(t, 1, 2, 1, 1000, 12000, 100)
					if kind == "signed_wrong_genesis" {
						sell["order"].(map[string]any)["genesis_hash"] = strings.Repeat("0", 64)
						sell = f.signOrder(t, 0, sell["order"].(map[string]any))
					}
					if kind == "signed_other_fee_genesis" {
						other, _ := localFixtureInputs(t, 25-fee)
						sell["order"].(map[string]any)["genesis_hash"] = localSHA(other.Genesis)
						sell = f.signOrder(t, 0, sell["order"].(map[string]any))
					}
					if kind == "signature_tamper" {
						raw := ex.Raw(sell, "signature")
						raw[0] ^= 1
						sell["signature"] = base64Text(raw)
					}
					fill := f.fill(t, sell, buy, 1000, 10000, 1)
					if kind == "other_fee" {
						fill["fee_policy_version"] = "2"
						if fee == 25 {
							fill["fee_policy_version"] = "1"
						}
					}
					wire := f.batch(t, []map[string]any{sell, buy}, []map[string]any{fill})
					if kind == "old_batch_genesis" {
						m, err := contract.Decode("BatchV1", wire)
						mustTest(t, err)
						m["genesis_hash"] = strings.Repeat("0", 64)
						wire = seal(t, m)
					}
					before := f.exchangeState(t)
					r := f.submit(t, wire)
					want := "CONTEXT_MISMATCH"
					if kind == "signature_tamper" {
						want = "INVALID_SIGNATURE"
					}
					if kind == "other_fee" {
						want = "MARKET_LIMIT"
					}
					requireCode(t, r, want)
					if !reflect.DeepEqual(before, f.exchangeState(t)) {
						t.Fatal("rejected settlement changed exchange assets")
					}
					localEvidence(t, "signed-vector.json", map[string]any{"batch_wire": wire, "context": g.Context, "result": r, "expected_state_diff": map[string]any{}})
				})
			}
			requireCode(t, f.submit(t, f.demo(t)), "")
		})
	}
}
func base64Text(raw []byte) string { b, _ := json.Marshal(raw); return string(b[1 : len(b)-1]) }

func TestLocalDemoStandardBoundary(t *testing.T) {
	in, _ := localFixtureInputs(t, 0)
	doc, err := cmttypes.GenesisDocFromJSON(in.Genesis)
	mustTest(t, err)
	if _, err := DecodeS3Genesis(doc.AppState); err == nil {
		t.Fatal("standard genesis decoder accepted dev hash")
	}
	f := newS3Fixture(t, 2, 0, false)
	defer f.db.Close()
	// Even a caller holding the component constructor cannot adopt a standard DB.
	if _, err := newLocalDemoComponent(f.db, log.NewNopLogger(), in); err == nil {
		t.Fatal("development reopened standard DB")
	}
	if f.a.Exchange.Context(f.ctx(t))["service_schema"] != "s3/1" {
		t.Fatal("standard context changed")
	}
	requireCode(t, f.submit(t, f.demo(t)), "INSUFFICIENT_CONFIRMED_BALANCE")
	t.Logf("standard hashes remain %s / %s; genesis=%s", ex.S3ContractHash, ex.S3ConfigHash, hex.EncodeToString(f.hash))
}

func TestLocalDemoReceiptRejectsResealedContext(t *testing.T) {
	f, _ := newLocalFixture(t, 25)
	f.deposit(t, 0, ex.Base, "10000000")
	f.deposit(t, 1, ex.Quote, "100000000")
	requireCode(t, f.submit(t, f.demo(t)), "")
	for _, field := range []string{"service_schema", "contract_hash", "config_hash", "genesis_hash"} {
		t.Run(field, func(t *testing.T) {
			ctx := f.ctx(t)
			k := f.a.Exchange
			original := bytes.Clone(k.S3Get(ctx, "receipt/1"))
			digest := bytes.Clone(k.S3Get(ctx, "receipt/1/digest"))
			defer k.S3Set(ctx, "receipt/1", original)
			defer k.S3Set(ctx, "receipt/1/digest", digest)
			var r ex.S3Receipt
			mustTest(t, json.Unmarshal(original, &r))
			r.Context[field] = "wrong"
			if field == "service_schema" {
				r.Context[field] = "s3/1"
			}
			raw := localMarshal(t, r)
			k.S3Set(ctx, "receipt/1", raw)
			k.S3Set(ctx, "receipt/1/digest", []byte(localSHA(raw)))
			_, err := k.StoredReceipt(ctx, 1, k.Last(ctx))
			if err == nil || !strings.Contains(err.Error(), "RECEIPT_INCONSISTENCY") {
				t.Fatalf("resealed context accepted: %v", err)
			}
		})
	}
}

func TestLocalDemoFullManifestShape(t *testing.T) {
	// Synthetic descriptors exercise the production entry point's structural
	// validation, never an assertion that these five components were approved.
	in, _ := localFixtureInputs(t, 0)
	rebindLocalManifest(t, &in, func(m *localManifest) {
		m.Scope = "REVIEWED_RUNTIME"
		for _, name := range []string{"exchange", "settlement", "wallet", "sre"} {
			p := "chain/local-demo/components/" + name + ".json"
			in.Files[p] = localMarshal(t, map[string]any{"head": strings.Repeat("1", 40), "tree": strings.Repeat("2", 40), "implementation_settings": map[string]string{"scope": "TEST_ONLY_NOT_APPROVED"}})
			m.Components[name] = p
			m.Files[p] = localSHA(in.Files[p])
		}
	})
	var m localManifest
	mustTest(t, json.Unmarshal(in.RuntimeManifest, &m))
	changeLocalGenesis(t, &in, func(_ *cmttypes.GenesisDoc, g *S3Genesis) { g.ContractHash = m.ContractSHA })
	ctx, err := ValidateLocalDemo(in)
	mustTest(t, err)
	if ctx["service_schema"] != "s3/3" {
		t.Fatal("wrong runtime context")
	}
	db := dbm.NewMemDB()
	defer db.Close()
	a, err := NewLocalDemo(db, log.NewNopLogger(), in)
	mustTest(t, err)
	_, err = a.InitChain(localInitRequest(t, in))
	mustTest(t, err)
	bad := copyLocalInputs(t, in)
	rebindLocalManifest(t, &bad, func(m *localManifest) { delete(m.Components, "wallet") })
	if _, err := ValidateLocalDemo(bad); err == nil {
		t.Fatal("missing component accepted")
	}
	t.Log("synthetic complete manifest shape PASS; actual runtime approval/integration NOT_RUN")
}
