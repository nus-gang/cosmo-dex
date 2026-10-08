//go:build dev_local_demo

package localkeys

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"time"

	app "github.com/nus-gang/cosmo-dex/chain/app"
)

const cValidated = "VALIDATED_INPUT_BYTES_ONLY; RUNTIME_APPROVAL_NOT_ESTABLISHED; durable_ack=false\n"

type Initialization struct {
	in          app.LocalDemoInputs
	nodes       []Material
	authorities *Authorities
	bundle      []byte
	cValidated  bool
	closed      bool
}

type PublicInitialization struct {
	Schema         string   `json:"schema"`
	FeeBPS         string   `json:"fee_bps"`
	Root           string   `json:"root"`
	AuthorityRoot  string   `json:"authority_root"`
	Homes          []string `json:"homes"`
	NodeIDs        []string `json:"node_ids"`
	GenesisSHA256  string   `json:"genesis_sha256"`
	GuardSHA256    string   `json:"guard_sha256"`
	RuntimePin     string   `json:"runtime_pin"`
	CValidated     bool     `json:"c_semantic_validation_verified"`
	ServiceStarted bool     `json:"service_started"`
}

// PrepareInitialization creates fresh secrets in memory and binds their public
// values to the exact reviewed manifest/profile. It performs no filesystem IO.
func PrepareInitialization(base app.LocalDemoInputs, users [][]byte, fee, runUUID string, at time.Time) (*Initialization, error) {
	reject := errors.New("INITIALIZATION_REJECTED")
	if len(base.Guard) != 0 || len(base.Genesis) != 0 || (fee != "0" && fee != "25") || at.IsZero() || at.Location() != time.UTC {
		return nil, reject
	}
	if len(users) != 2 {
		return nil, reject
	}
	for _, key := range users {
		if len(key) != 1952 {
			return nil, reject
		}
	}
	var manifest struct {
		Contract string `json:"contract_sha256"`
	}
	if json.Unmarshal(base.RuntimeManifest, &manifest) != nil {
		return nil, reject
	}
	authorities, err := GenerateAuthorities()
	if err != nil {
		return nil, reject
	}
	nodes, err := GenerateFour()
	if err != nil {
		authorities.Destroy()
		return nil, reject
	}
	prepared := &Initialization{in: base, nodes: nodes, authorities: authorities}
	ok := false
	defer func() {
		if !ok {
			prepared.Destroy()
		}
	}()
	operators, admin, err := authorities.Public()
	if err != nil {
		return nil, reject
	}
	state, err := AppStateBytes(users, operators, admin, fee, manifest.Contract, digest(base.EffectiveProfile))
	if err != nil {
		return nil, reject
	}
	prepared.in.Genesis, err = GenesisBytes(nodes, at, state)
	if err != nil {
		return nil, reject
	}
	prepared.in.Guard, _, err = PrepareGuard(prepared.in, runUUID, "fee"+fee)
	if err != nil {
		return nil, reject
	}
	prepared.bundle, err = InputBundle(prepared.in)
	if err != nil {
		return nil, reject
	}
	ok = true
	return prepared, nil
}

type boundedWriter struct {
	buf  bytes.Buffer
	max  int
	over bool
}

func (w *boundedWriter) Write(p []byte) (int, error) {
	n := len(p)
	remaining := w.max - w.buf.Len()
	if remaining > 0 {
		if len(p) > remaining {
			w.buf.Write(p[:remaining])
		} else {
			w.buf.Write(p)
		}
	}
	if n > remaining {
		w.over = true
	}
	return n, nil
}

// ValidateC invokes the reviewed offline C validator on the exact B-produced
// bundle. Only this method can open the subsequent publication gate.
func (p *Initialization) ValidateC(ctx context.Context, validator, scratch string) error {
	reject := errors.New("C_VALIDATION_REJECTED")
	if p == nil || p.closed || p.cValidated || ctx == nil || !filepath.IsAbs(validator) || filepath.Clean(validator) != validator {
		return reject
	}
	if err := privateDirectory(scratch); err != nil {
		return reject
	}
	resolved, err := filepath.EvalSymlinks(validator)
	if err != nil || resolved != validator {
		return reject
	}
	info, err := os.Lstat(validator)
	stat, ok := infoSys(info)
	if err != nil || !ok || !info.Mode().IsRegular() || stat.Nlink != 1 || stat.Uid != uint32(os.Geteuid()) || info.Mode().Perm()&0111 == 0 {
		return reject
	}
	bundle, err := os.CreateTemp(scratch, ".initializer-bundle-")
	if err != nil {
		return reject
	}
	bundleName := bundle.Name()
	profileName := bundleName + ".profile"
	cleanup := func() error {
		e1 := os.Remove(bundleName)
		e2 := os.Remove(profileName)
		d, e3 := os.Open(scratch)
		if e3 == nil {
			e3 = d.Sync()
			d.Close()
		}
		if e1 != nil || e2 != nil || e3 != nil {
			return reject
		}
		return nil
	}
	if bundle.Chmod(0600) != nil || writeAndSync(bundle, p.bundle) != nil || bundle.Close() != nil {
		bundle.Close()
		_ = os.Remove(bundleName)
		return reject
	}
	profile, err := os.OpenFile(profileName, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil || writeAndSync(profile, p.in.EffectiveProfile) != nil || profile.Close() != nil {
		if profile != nil {
			profile.Close()
		}
		_ = os.Remove(bundleName)
		_ = os.Remove(profileName)
		return reject
	}
	cmd := exec.CommandContext(ctx, validator, "validate", "--input-set", bundleName,
		"--local-demo-profile", profileName, "--runtime-pin", p.in.ApprovedRuntimeSHA256,
		"--acknowledge-unproven-space")
	cmd.Env = []string{"PATH=/usr/bin:/bin", "LC_ALL=C"}
	cmd.Stdin = nil
	stdout, stderr := &boundedWriter{max: 256}, &boundedWriter{max: 4096}
	cmd.Stdout, cmd.Stderr = stdout, stderr
	err = cmd.Run()
	cleanErr := cleanup()
	if err != nil || cleanErr != nil || stdout.over || stderr.over || stdout.buf.String() != cValidated {
		return reject
	}
	p.cValidated = true
	return nil
}

func infoSys(info os.FileInfo) (*syscall.Stat_t, bool) {
	if info == nil {
		return nil, false
	}
	st, ok := info.Sys().(*syscall.Stat_t)
	return st, ok
}
func writeAndSync(file *os.File, raw []byte) error {
	for len(raw) > 0 {
		n, err := file.Write(raw)
		if err != nil || n <= 0 {
			return errors.New("WRITE_REJECTED")
		}
		raw = raw[n:]
	}
	return file.Sync()
}

func (p *Initialization) Destroy() {
	if p == nil || p.closed {
		return
	}
	if p.authorities != nil {
		p.authorities.Destroy()
	}
	for i := range p.nodes {
		p.nodes[i].destroy()
	}
	clear(p.bundle)
	p.bundle = nil
	p.closed = true
}

func privateDirectory(path string) error {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return errors.New("PRIVATE_DIRECTORY_REQUIRED")
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil || resolved != path {
		return errors.New("PRIVATE_DIRECTORY_REQUIRED")
	}
	info, err := os.Lstat(path)
	stat, ok := infoSys(info)
	if err != nil || !ok || !info.IsDir() || info.Mode().Perm() != 0700 || stat.Uid != uint32(os.Geteuid()) {
		return errors.New("PRIVATE_DIRECTORY_REQUIRED")
	}
	return nil
}

var _ io.Writer = (*boundedWriter)(nil)
