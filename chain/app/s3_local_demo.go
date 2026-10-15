//go:build dev_local_demo

package app

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"path"
	"reflect"
	"regexp"
	"sort"
	"strings"

	"cosmossdk.io/log/v2"
	abci "github.com/cometbft/cometbft/abci/types"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
)

// These pins are the dormant NUS-54 Security -> QA input, not a runtime approval.
const localCandidateSHA = "90169d322336a0c0de9bc6c48725d528d42fe74c78ea5b596fc7e059d747dda2"
const localBaselineSHA = "3ff69e73057a2bb6dcff64820123d520b9ad3e5637abbd1ad7d38b8c1a49eb97"
const localCandidatePath = "proposals/s3-local-dev-v1/"
const localProfileID = "s3-dev-local-v1"
const localPublicPath = "proposals/s3-local-account-receipt-v1/"
const localPublicManifestSHA = "5e911b9a5fc750c702c9c8cde09dcfc2c56106a7e0757f1d7ad2e839019b50fa"
const localPublicSchemaSHA = "2bbb848b836c8d15f2732b481f78be2e28b0cbc2b7c783971bc593747d120b6b"
const localPublicVersion = "s3-dev-local-account/1"

// LocalDemoInputs contains exact bytes from an independently pinned input set.
// The caller must obtain ApprovedRuntimeSHA256 from the review handoff, not from
// an untrusted manifest. Byte verification cannot establish human approval.
// Files is a sealed source snapshot, not the current mutable checkout. In
// particular, inherited rc3 locks and component implementation settings are
// distinct inputs; the inherited manifest must never be resealed.
// This adapter does not create homes, guards, WALs, services or an ACK gate.
// Their path/inode/lock/fsync lifecycle belongs to the launcher/store owner.
type LocalDemoInputs struct {
	ApprovedRuntimeSHA256    string            `json:"approved_runtime_sha256"`
	RuntimeManifest          []byte            `json:"runtime_manifest"`
	Files                    map[string][]byte `json:"files"`
	EffectiveProfile         []byte            `json:"effective_profile"`
	Guard                    []byte            `json:"guard"`
	Genesis                  []byte            `json:"genesis"`
	AcknowledgeUnprovenSpace bool              `json:"acknowledge_unproven_space"`
}

type localManifest struct {
	Format            string            `json:"format"`
	Scope             string            `json:"scope"`
	CandidateSHA      string            `json:"candidate_manifest_sha256"`
	PublicManifestSHA string            `json:"public_receipt_manifest_sha256"`
	PublicSchemaSHA   string            `json:"public_receipt_schema_sha256"`
	PublicVersion     string            `json:"public_receipt_version"`
	ContractSHA       string            `json:"contract_sha256"`
	Files             map[string]string `json:"files_sha256"`
	Components        map[string]string `json:"components"`
}
type localGuard struct {
	Envelope     string            `json:"envelope_version"`
	Profile      string            `json:"profile_id"`
	CandidateSHA string            `json:"candidate_manifest_sha256"`
	RuntimeSHA   string            `json:"runtime_manifest_sha256"`
	EffectiveSHA string            `json:"effective_profile_sha256"`
	RunUUID      string            `json:"run_uuid"`
	FeeProfile   string            `json:"fee_profile"`
	Context      map[string]string `json:"context"`
}

func localSHA(raw []byte) string { sum := sha256.Sum256(raw); return hex.EncodeToString(sum[:]) }
func localAggregate(files map[string]string) string {
	names := make([]string, 0, len(files))
	for name := range files {
		names = append(names, name)
	}
	sort.Strings(names)
	var raw strings.Builder
	for _, name := range names {
		fmt.Fprintf(&raw, "%s  %s\n", files[name], name)
	}
	return localSHA([]byte(raw.String()))
}

var localHash = regexp.MustCompile(`^[0-9a-f]{64}$`)
var localGitHash = regexp.MustCompile(`^[0-9a-f]{40}$`)
var localUUID = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`)

// Strict JSON also checks exact spelling and presence (encoding/json otherwise
// accepts case-insensitive struct fields and silently defaults missing fields).
func localJSON(raw []byte, out any, fields ...string) error {
	if len(raw) == 0 || len(raw) > 2*1024*1024 {
		return fmt.Errorf("LOCAL_INPUT_SIZE")
	}
	if err := strictJSON(raw, out); err != nil {
		return err
	}
	var obj map[string]json.RawMessage
	if err := json.Unmarshal(raw, &obj); err != nil || len(obj) != len(fields) {
		return fmt.Errorf("LOCAL_INPUT_FIELDS")
	}
	for _, f := range fields {
		if obj[f] == nil || bytes.Equal(obj[f], []byte("null")) {
			return fmt.Errorf("LOCAL_INPUT_FIELDS")
		}
	}
	return nil
}

func validateLocalDemo(in LocalDemoInputs, component bool) (*ex.S3Binding, *cmttypes.GenesisDoc, error) {
	fail := func(code string) (*ex.S3Binding, *cmttypes.GenesisDoc, error) {
		return nil, nil, fmt.Errorf("%s", code)
	}
	if !in.AcknowledgeUnprovenSpace || len(in.EffectiveProfile) == 0 {
		return fail("LOCAL_DEMO_OPT_IN_REQUIRED")
	}
	if !localHash.MatchString(in.ApprovedRuntimeSHA256) || localSHA(in.RuntimeManifest) != in.ApprovedRuntimeSHA256 {
		return fail("RUNTIME_MANIFEST_MISMATCH")
	}
	var m localManifest
	if err := localJSON(in.RuntimeManifest, &m, "format", "scope", "candidate_manifest_sha256", "public_receipt_manifest_sha256", "public_receipt_schema_sha256", "public_receipt_version", "contract_sha256", "files_sha256", "components"); err != nil {
		return nil, nil, err
	}
	scope := "REVIEWED_RUNTIME"
	requiredComponents := []string{"chain", "exchange", "settlement", "wallet", "sre"}
	if component {
		scope = "COMPONENT_FIXTURE"
		requiredComponents = []string{"chain"}
	}
	if m.Format != "s3-dev-local-runtime/1" || m.Scope != scope || m.CandidateSHA != localCandidateSHA {
		return fail("RUNTIME_MANIFEST_SCOPE")
	}
	if m.PublicManifestSHA != localPublicManifestSHA || m.PublicSchemaSHA != localPublicSchemaSHA || m.PublicVersion != localPublicVersion {
		return fail("PUBLIC_RECEIPT_PIN_MISMATCH")
	}
	if len(m.Files) > 512 || len(in.Files) != len(m.Files) || len(m.Components) != len(requiredComponents) {
		return fail("RUNTIME_MANIFEST_FILES")
	}
	total := 0
	for name, digest := range m.Files {
		// No aliases, platform separators, hidden runtime paths, or arbitrary files.
		if name == "" || path.Clean(name) != name || strings.HasPrefix(name, "/") || strings.HasPrefix(name, "../") || strings.ContainsAny(name, "\\\x00\r\n") || !localHash.MatchString(digest) {
			return fail("RUNTIME_MANIFEST_FILES")
		}
		raw, ok := in.Files[name]
		total += len(raw)
		if !ok || len(raw) > 16*1024*1024 || total > 32*1024*1024 || localSHA(raw) != digest {
			return fail("RUNTIME_FILE_MISMATCH")
		}
	}
	if !localHash.MatchString(m.ContractSHA) || m.ContractSHA != localAggregate(m.Files) {
		return fail("CONTRACT_HASH_MISMATCH")
	}
	candidateRaw := in.Files[localCandidatePath+"MANIFEST.json"]
	baselineRaw := in.Files["protocol/s3/manifest.json"]
	if localSHA(candidateRaw) != localCandidateSHA || localSHA(baselineRaw) != localBaselineSHA {
		return fail("INHERITED_MANIFEST_MISMATCH")
	}
	// These two manifests have pinned bytes, so no extensible input schema is
	// inferred from them. Every listed source byte must be present and unchanged.
	var candidate struct {
		Files map[string]string `json:"files_sha256"`
	}
	var baseline struct {
		Files    map[string]string `json:"files_sha256"`
		Contract string            `json:"contract_sha256"`
	}
	if json.Unmarshal(candidateRaw, &candidate) != nil || json.Unmarshal(baselineRaw, &baseline) != nil || baseline.Contract != localAggregate(baseline.Files) {
		return fail("INHERITED_MANIFEST_MISMATCH")
	}
	expected := map[string]string{localCandidatePath + "MANIFEST.json": localCandidateSHA, "protocol/s3/manifest.json": localBaselineSHA}
	for name, digest := range baseline.Files {
		expected[name] = digest
	}
	for name, digest := range candidate.Files {
		expected[localCandidatePath+name] = digest
	}
	// The public overlay is an approved, self-excluded source manifest. Its
	// listed paths are already repo-relative (unlike the older local proposal).
	// Pin the manifest itself, then include it and every listed file exactly
	// once in the runtime aggregate. There is no legacy-manifest fallback.
	publicRaw := in.Files[localPublicPath+"MANIFEST.json"]
	if localSHA(publicRaw) != localPublicManifestSHA {
		return fail("PUBLIC_RECEIPT_MANIFEST_MISMATCH")
	}
	var public struct {
		SelfExcluded bool              `json:"self_excluded"`
		Aggregate    string            `json:"candidate_files_sha256"`
		SchemaSHA    string            `json:"public_schema_sha256"`
		Files        map[string]string `json:"files_sha256"`
		Inherited    map[string]string `json:"inherited_files_sha256"`
	}
	if json.Unmarshal(publicRaw, &public) != nil || !public.SelfExcluded || public.SchemaSHA != localPublicSchemaSHA || public.Files[localPublicPath+"schema.json"] != localPublicSchemaSHA || public.Files[localPublicPath+"MANIFEST.json"] != "" || public.Aggregate != localAggregate(public.Files) || !reflect.DeepEqual(public.Inherited, expected) {
		return fail("PUBLIC_CONTRACT_AGGREGATE_MISMATCH")
	}
	for name, digest := range public.Files {
		if old, ok := expected[name]; ok && old != digest {
			return fail("PUBLIC_CONTRACT_OVERLAP")
		}
		expected[name] = digest
	}
	expected[localPublicPath+"MANIFEST.json"] = localPublicManifestSHA
	for _, name := range requiredComponents {
		file := "chain/local-demo/components/" + name + ".json"
		if m.Components[name] != file || expected[file] != "" {
			return fail("COMPONENT_BINDING_MISMATCH")
		}
		var c struct {
			Head     string            `json:"head"`
			Tree     string            `json:"tree"`
			Settings map[string]string `json:"implementation_settings"`
		}
		if err := localJSON(in.Files[file], &c, "head", "tree", "implementation_settings"); err != nil {
			return nil, nil, err
		}
		if !localGitHash.MatchString(c.Head) || !localGitHash.MatchString(c.Tree) || len(c.Settings) == 0 {
			return fail("COMPONENT_BINDING_MISMATCH")
		}
		expected[file] = m.Files[file]
	}
	if !reflect.DeepEqual(expected, m.Files) {
		return fail("INHERITED_FILE_SET_MISMATCH")
	}
	var guard localGuard
	if err := localJSON(in.Guard, &guard, "envelope_version", "profile_id", "candidate_manifest_sha256", "runtime_manifest_sha256", "effective_profile_sha256", "run_uuid", "fee_profile", "context"); err != nil {
		return nil, nil, err
	}
	canonical, err := canonicalJSON(guard)
	if err != nil || !bytes.Equal(canonical, in.Guard) {
		return fail("NON_CANONICAL_GUARD")
	}
	if guard.Envelope != "s3-dev-local/1" || guard.Profile != localProfileID || guard.CandidateSHA != localCandidateSHA || guard.RuntimeSHA != in.ApprovedRuntimeSHA256 || guard.EffectiveSHA != localSHA(in.EffectiveProfile) || !localUUID.MatchString(guard.RunUUID) {
		return fail("GUARD_MISMATCH")
	}
	fee := uint64(0)
	switch guard.FeeProfile {
	case "fee0":
	case "fee25":
		fee = 25
	default:
		return fail("INVALID_FEE_PROFILE")
	}
	if !bytes.Equal(in.EffectiveProfile, in.Files[localCandidatePath+"effective-profile-"+guard.FeeProfile+".json"]) {
		return fail("EFFECTIVE_PROFILE_MISMATCH")
	}
	if len(in.Genesis) == 0 || len(in.Genesis) > 1024*1024 {
		return fail("LOCAL_INPUT_SIZE")
	}
	// Validate duplicate JSON keys before the Comet decoder consumes the document.
	var obj any
	if err := strictJSON(in.Genesis, &obj); err != nil {
		return nil, nil, err
	}
	genesis, err := cmttypes.GenesisDocFromJSON(in.Genesis)
	if err != nil {
		return nil, nil, err
	}
	// ValidateAndComplete may fill defaults. Require explicit consensus values
	// below and bind InitChain to this decoded document and exact app-state bytes.
	if genesis.ChainID != ex.S3ChainID || genesis.InitialHeight != 1 || len(genesis.Validators) != 4 || genesis.ConsensusParams == nil {
		return fail("S3_CONSENSUS_PROFILE")
	}
	p := genesis.ConsensusParams
	if p.Block.MaxBytes != 1048576 || p.Block.MaxGas != 20000000 || p.Evidence.MaxBytes != 65536 {
		return fail("S3_CONSENSUS_PROFILE")
	}
	wantContext := map[string]string{"service_schema": "s3/3", "chain_id": ex.S3ChainID, "genesis_hash": localSHA(in.Genesis), "contract_hash": m.ContractSHA, "config_hash": guard.EffectiveSHA, "market_id": ex.S3Market, "market_config_version": "1"}
	if !reflect.DeepEqual(guard.Context, wantContext) {
		return fail("CONTEXT_MISMATCH")
	}
	binding := &ex.S3Binding{ContractHash: m.ContractSHA, ConfigHash: guard.EffectiveSHA, FeeBPS: fee, Guard: string(in.Guard)}
	if _, err := decodeS3Genesis(genesis.AppState, binding); err != nil {
		return nil, nil, err
	}
	return binding, genesis, nil
}

// ValidateLocalDemo checks a complete reviewed runtime input set without starting
// a service. Component fixtures are deliberately rejected by this entry point.
func ValidateLocalDemo(in LocalDemoInputs) (map[string]string, error) {
	binding, _, err := validateLocalDemo(in, false)
	if err != nil {
		return nil, err
	}
	var guard localGuard
	if err := json.Unmarshal([]byte(binding.Guard), &guard); err != nil {
		return nil, err
	}
	return guard.Context, nil
}

func NewLocalDemo(db dbm.DB, logger log.Logger, in LocalDemoInputs) (*App, error) {
	return newLocalDemo(db, logger, in, false)
}

func newLocalDemo(db dbm.DB, logger log.Logger, in LocalDemoInputs, component bool) (*App, error) {
	binding, genesis, err := validateLocalDemo(in, component)
	if err != nil {
		return nil, err
	}
	hash := sha256.Sum256(in.Genesis)
	a, err := newForChain(db, hash[:], logger, ex.S3ChainID, binding)
	if err != nil {
		return nil, err
	}
	a.validateS3Init = func(req *abci.RequestInitChain) error {
		if req.ChainId != genesis.ChainID || req.InitialHeight != genesis.InitialHeight || !req.Time.Equal(genesis.GenesisTime) || !bytes.Equal(req.AppStateBytes, genesis.AppState) || req.ConsensusParams == nil || !reflect.DeepEqual(*req.ConsensusParams, genesis.ConsensusParams.ToProto()) || len(req.Validators) != len(genesis.Validators) {
			return fmt.Errorf("INIT_GENESIS_MISMATCH")
		}
		// Comet sorts validator updates. Bind the exact key/power set, not
		// the incidental array order of RequestInitChain.
		validators := map[string]int64{}
		for _, v := range genesis.Validators {
			if v.PubKey.Type() != "ed25519" || v.Power <= 0 || validators[string(v.PubKey.Bytes())] != 0 {
				return fmt.Errorf("INIT_GENESIS_MISMATCH")
			}
			validators[string(v.PubKey.Bytes())] = v.Power
		}
		for _, v := range req.Validators {
			key := string(v.PubKey.GetEd25519())
			if len(key) != 32 || v.Power <= 0 || validators[key] != v.Power {
				return fmt.Errorf("INIT_GENESIS_MISMATCH")
			}
			delete(validators, key)
		}
		return nil
	}
	return a, nil
}
