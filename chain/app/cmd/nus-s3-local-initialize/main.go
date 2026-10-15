//go:build dev_local_demo

// nus-s3-local-initialize creates fresh fee0/fee25 runtime inputs without
// starting a service. It publishes only after approved B and offline C semantic
// validation. Organizational runtime approval remains an external prerequisite.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"regexp"
	"strings"
	"syscall"
	"time"

	app "github.com/nus-gang/cosmo-dex/chain/app"
	"github.com/nus-gang/cosmo-dex/chain/app/internal/localkeys"
)

const failure = "LOCAL_INITIALIZATION_REJECTED_PRESERVE_PARTIAL_OUTPUT"

type options struct {
	source, profile, pin, users, output, scratch, runUUID, genesisTime, fee, validator string
	ack                                                                                bool
}

func parse(args []string) (options, error) {
	var o options
	if len(args) == 0 || args[0] != "create" {
		return o, errors.New("MODE")
	}
	f := flag.NewFlagSet("create", flag.ContinueOnError)
	f.SetOutput(io.Discard)
	f.StringVar(&o.source, "source-input", "", "")
	f.StringVar(&o.profile, "effective-profile", "", "")
	f.StringVar(&o.pin, "runtime-pin", "", "")
	f.StringVar(&o.users, "user-public-keys", "", "")
	f.StringVar(&o.output, "output", "", "")
	f.StringVar(&o.scratch, "scratch", "", "")
	f.StringVar(&o.runUUID, "run-uuid", "", "")
	f.StringVar(&o.genesisTime, "genesis-time", "", "")
	f.StringVar(&o.fee, "fee-bps", "", "")
	f.StringVar(&o.validator, "c-validator", "", "")
	var localProfile, publicationGate string
	f.StringVar(&localProfile, "local-demo-profile", "", "")
	f.StringVar(&publicationGate, "publication-gate", "", "")
	f.BoolVar(&o.ack, "acknowledge-unproven-space", false, "")
	seen := map[string]bool{}
	for i := 1; i < len(args); i++ {
		name := args[i]
		if !strings.HasPrefix(name, "--") || strings.Contains(name, "=") || seen[name] {
			return o, errors.New("OPTION")
		}
		seen[name] = true
		if name != "--acknowledge-unproven-space" {
			i++
			if i >= len(args) || strings.HasPrefix(args[i], "--") {
				return o, errors.New("VALUE")
			}
		}
	}
	if f.Parse(args[1:]) != nil || f.NArg() != 0 || !o.ack || localProfile != "s3-dev-local/1" || publicationGate != "stdin" ||
		(o.fee != "0" && o.fee != "25") || !regexp.MustCompile(`^[0-9a-f]{64}$`).MatchString(o.pin) {
		return o, errors.New("INPUT")
	}
	for _, path := range []string{o.source, o.profile, o.users, o.output, o.scratch, o.validator} {
		if !filepath.IsAbs(path) || filepath.Clean(path) != path {
			return o, errors.New("PATH")
		}
	}
	return o, nil
}

func uniqueJSON(raw []byte) error {
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	var value func() error
	value = func() error {
		token, err := d.Token()
		if err != nil {
			return err
		}
		switch token {
		case json.Delim('{'):
			seen := map[string]bool{}
			for d.More() {
				key, err := d.Token()
				if err != nil {
					return err
				}
				name, ok := key.(string)
				if !ok || seen[name] {
					return errors.New("JSON")
				}
				seen[name] = true
				if err = value(); err != nil {
					return err
				}
			}
			end, err := d.Token()
			if err != nil || end != json.Delim('}') {
				return errors.New("JSON")
			}
		case json.Delim('['):
			for d.More() {
				if err := value(); err != nil {
					return err
				}
			}
			end, err := d.Token()
			if err != nil || end != json.Delim(']') {
				return errors.New("JSON")
			}
		}
		return nil
	}
	if err := value(); err != nil {
		return err
	}
	if _, err := d.Token(); err != io.EOF {
		return errors.New("JSON")
	}
	return nil
}

func read(path string, max int64) ([]byte, error) {
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil || resolved != path {
		return nil, errors.New("FILE")
	}
	before, err := os.Lstat(path)
	if err != nil {
		return nil, errors.New("FILE")
	}
	stat, ok := before.Sys().(*syscall.Stat_t)
	if !ok || !before.Mode().IsRegular() || stat.Nlink != 1 || before.Size() < 1 || before.Size() > max {
		return nil, errors.New("FILE")
	}
	fd, err := syscall.Open(path, syscall.O_RDONLY|syscall.O_NOFOLLOW, 0)
	if err != nil {
		return nil, errors.New("FILE")
	}
	file := os.NewFile(uintptr(fd), path)
	defer file.Close()
	after, err := file.Stat()
	if err != nil || !os.SameFile(before, after) {
		return nil, errors.New("FILE")
	}
	raw, err := io.ReadAll(io.LimitReader(file, max+1))
	if err != nil || int64(len(raw)) > max {
		return nil, errors.New("FILE")
	}
	return raw, nil
}

func sourceInput(o options) (app.LocalDemoInputs, [][]byte, time.Time, error) {
	var in app.LocalDemoInputs
	reject := errors.New("INPUT")
	raw, err := read(o.source, 48<<20)
	if err != nil || uniqueJSON(raw) != nil {
		return in, nil, time.Time{}, reject
	}
	var source struct {
		Manifest []byte            `json:"runtime_manifest"`
		Files    map[string][]byte `json:"files"`
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || len(fields) != 2 || fields["runtime_manifest"] == nil || fields["files"] == nil || json.Unmarshal(raw, &source) != nil {
		return in, nil, time.Time{}, reject
	}
	profile, err := read(o.profile, 1<<20)
	if err != nil {
		return in, nil, time.Time{}, reject
	}
	usersRaw, err := read(o.users, 16<<10)
	if err != nil {
		return in, nil, time.Time{}, reject
	}
	users, err := localkeys.ParseUserPublicKeys(usersRaw)
	if err != nil {
		return in, nil, time.Time{}, reject
	}
	at, err := time.Parse(time.RFC3339Nano, o.genesisTime)
	if err != nil || at.Location() != time.UTC || at.Format(time.RFC3339Nano) != o.genesisTime {
		return in, nil, time.Time{}, reject
	}
	in = app.LocalDemoInputs{ApprovedRuntimeSHA256: o.pin, RuntimeManifest: source.Manifest, Files: source.Files,
		EffectiveProfile: profile, AcknowledgeUnprovenSpace: o.ack}
	return in, users, at, nil
}

func run(args []string, gate io.ReadCloser, ready io.Writer) ([]byte, error) {
	o, err := parse(args)
	if err != nil {
		return nil, err
	}
	in, users, at, err := sourceInput(o)
	if err != nil {
		return nil, err
	}
	defer func() {
		for _, key := range users {
			clear(key)
		}
	}()
	prepared, err := localkeys.PrepareInitialization(in, users, o.fee, o.runUUID, at)
	if err != nil {
		return nil, err
	}
	defer prepared.Destroy()
	signalContext, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	ctx, cancel := context.WithTimeout(signalContext, 60*time.Second)
	defer cancel()
	if err = prepared.ValidateC(ctx, o.validator, o.scratch); err != nil {
		return nil, err
	}
	// Sequencing only: the reviewed parent authenticates fresh independent
	// approvals and checks its staged bytes after READY, before sending PUBLISH.
	if err = awaitPublication(ctx, gate, ready, 30*time.Second); err != nil {
		return nil, err
	}
	report, err := prepared.Publish(o.output)
	if err != nil {
		return nil, err
	}
	return json.Marshal(report)
}

func main() {
	report, err := run(os.Args[1:], os.Stdin, os.Stdout)
	if err != nil {
		fmt.Fprintln(os.Stderr, failure)
		os.Exit(2)
	}
	os.Stdout.Write(append(report, '\n'))
}
