//go:build dev_local_demo

package localkeys

import (
	"encoding/json"
	"errors"
	"path/filepath"
	"strconv"

	"golang.org/x/sys/unix"
)

func (m *Material) destroy() {
	if m == nil {
		return
	}
	for name, raw := range m.files {
		clear(raw)
		delete(m.files, name)
	}
}

func mkdirPrivate(parent int, name string) (int, error) {
	if name == "" || filepath.Base(name) != name || name == "." || name == ".." || unix.Mkdirat(parent, name, 0700) != nil || unix.Fsync(parent) != nil {
		return -1, errors.New("NEW_ROOT_REQUIRED")
	}
	fd, err := unix.Openat(parent, name, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return -1, errors.New("NEW_ROOT_REQUIRED")
	}
	return fd, nil
}

func writeAt(dir int, name string, raw []byte) error {
	return writeBoundedAt(dir, name, raw, 1<<20)
}

func writeBoundedAt(dir int, name string, raw []byte, limit int) error {
	if name == "" || filepath.Base(name) != name || len(raw) == 0 || len(raw) > limit {
		return errors.New("OUTPUT_REJECTED")
	}
	fd, err := unix.Openat(dir, name, unix.O_WRONLY|unix.O_CREAT|unix.O_EXCL|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0600)
	if err != nil {
		return errors.New("OUTPUT_EXISTS")
	}
	defer unix.Close(fd)
	for len(raw) > 0 {
		n, err := unix.Write(fd, raw)
		if err != nil || n <= 0 {
			return errors.New("OUTPUT_WRITE")
		}
		raw = raw[n:]
	}
	if unix.Fsync(fd) != nil {
		return errors.New("OUTPUT_SYNC")
	}
	return nil
}

func publishHome(parent int, name string, guard, genesis []byte, node Material) error {
	files := node.PrivateFiles()
	defer func() {
		for _, raw := range files {
			clear(raw)
		}
	}()
	if len(guard) == 0 || len(genesis) == 0 || len(files) != 3 {
		return errors.New("HOME_INPUT_REJECTED")
	}
	root, err := mkdirPrivate(parent, name)
	if err != nil {
		return err
	}
	defer unix.Close(root)
	if err = writeAt(root, "guard.dev.json", guard); err != nil || unix.Fsync(root) != nil {
		return errors.New("HOME_GUARD_SYNC")
	}
	config, err := mkdirPrivate(root, "config")
	if err != nil {
		return err
	}
	for file, raw := range map[string][]byte{"genesis.json": genesis, "priv_validator_key.json": files["priv_validator_key.json"], "node_key.json": files["node_key.json"]} {
		if err = writeAt(config, file, raw); err != nil {
			unix.Close(config)
			return err
		}
	}
	if unix.Fsync(config) != nil {
		unix.Close(config)
		return errors.New("HOME_SYNC")
	}
	unix.Close(config)
	data, err := mkdirPrivate(root, "data")
	if err != nil {
		return err
	}
	if err = writeAt(data, "priv_validator_state.json", files["priv_validator_state.json"]); err != nil {
		unix.Close(data)
		return err
	}
	if unix.Fsync(data) != nil {
		unix.Close(data)
		return errors.New("HOME_SYNC")
	}
	unix.Close(data)
	if unix.Fsync(root) != nil || unix.Fsync(parent) != nil {
		return errors.New("HOME_SYNC")
	}
	return nil
}

// Publish creates one new profile root after both B and C validation. Partial
// output is preserved on failure; the same root is never retried or replaced.
func (p *Initialization) Publish(root string) (PublicInitialization, error) {
	result := PublicInitialization{}
	if p == nil || p.closed || !p.cValidated || !filepath.IsAbs(root) || filepath.Clean(root) != root {
		return result, errors.New("PUBLICATION_REJECTED")
	}
	defer p.Destroy()
	parent := filepath.Dir(root)
	if err := privateDirectory(parent); err != nil {
		return result, errors.New("PUBLICATION_REJECTED")
	}
	parentFD, err := unix.Open(parent, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return result, errors.New("PUBLICATION_REJECTED")
	}
	defer unix.Close(parentFD)
	rootFD, err := mkdirPrivate(parentFD, filepath.Base(root))
	if err != nil {
		return result, err
	}
	defer unix.Close(rootFD)

	ids := make([]string, 4)
	homes := make([]string, 4)
	for i := range p.nodes {
		_, id := p.nodes[i].Public()
		ids[i] = string(id)
		homes[i] = filepath.Join(root, "validator-"+strconv.Itoa(i))
	}
	result = PublicInitialization{Schema: "s3-local-initialization/1", FeeBPS: feeFromGuard(p.in.Guard), Root: root,
		AuthorityRoot: filepath.Join(root, "authority"), Homes: homes, NodeIDs: ids,
		GenesisSHA256: digest(p.in.Genesis), GuardSHA256: digest(p.in.Guard), RuntimePin: p.in.ApprovedRuntimeSHA256,
		CValidated: true, ServiceStarted: false}

	// Authority publication consumes its seeds on every outcome.
	if err = p.authorities.Publish(result.AuthorityRoot); err != nil {
		return PublicInitialization{}, err
	}
	for i := range p.nodes {
		if err = publishHome(rootFD, "validator-"+strconv.Itoa(i), p.in.Guard, p.in.Genesis, p.nodes[i]); err != nil {
			return PublicInitialization{}, err
		}
	}
	// Preserve the exact B/C-validated transport and effective profile for the
	// later Chain startup and authoritative-snapshot C bootstrap. No C store or
	// synthetic observation is created here. Both files contain public data only.
	if err = writeBoundedAt(rootFD, "input.json", p.bundle, 48<<20); err != nil {
		return PublicInitialization{}, err
	}
	if err = writeAt(rootFD, "effective-profile.json", p.in.EffectiveProfile); err != nil {
		return PublicInitialization{}, err
	}
	report, err := json.Marshal(result)
	if err != nil {
		return PublicInitialization{}, errors.New("REPORT_REJECTED")
	}
	if err = writeAt(rootFD, "initialization.json", report); err != nil || unix.Fsync(rootFD) != nil || unix.Fsync(parentFD) != nil {
		return PublicInitialization{}, errors.New("REPORT_SYNC")
	}
	return result, nil
}

func feeFromGuard(raw []byte) string {
	var value struct {
		Fee string `json:"fee_profile"`
	}
	if json.Unmarshal(raw, &value) != nil {
		return ""
	}
	if value.Fee == "fee0" {
		return "0"
	}
	if value.Fee == "fee25" {
		return "25"
	}
	return ""
}
