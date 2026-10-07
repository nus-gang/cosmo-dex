//go:build dev_local_demo

package main

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

// Exercise the real Go lock against the Python observer in a separate process.
// No node, DB, RPC, or service is constructed.
func TestWriterPythonReleaseProbe(t *testing.T) {
	python, module := os.Getenv("NUS_PYTHON"), os.Getenv("NUS_PROBE_MODULE")
	if python == "" || module == "" {
		t.Skip("explicit Python and probe module required")
	}
	for _, fee := range []string{"fee0", "fee25"} {
		t.Run(fee, func(t *testing.T) {
			root := privateDir(t)
			held, err := lock(root)
			if err != nil {
				t.Fatal(err)
			}
			defer held.Close()
			path := filepath.Join(root, "writer.dev.lock")
			before, err := os.Stat(path)
			if err != nil {
				t.Fatal(err)
			}
			probe := func(mode string) {
				t.Helper()
				ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
				defer cancel()
				cmd := exec.CommandContext(ctx, python, "-B", "-c", `import sys
sys.path.insert(0, sys.argv[1])
import chain_writer_release as p
if sys.argv[3] == 'held':
    try: p.check(sys.argv[2])
    except ValueError as e: assert str(e) == p.ERROR
    else: raise AssertionError('Go writer was not excluded')
else:
    r = p.check(sys.argv[2])
    assert r['writer_lock_reacquired'] is True
    assert r['continuous_exclusion_verified'] is False
    assert r['cleanup_complete_verified'] is False
print('PROBE_PASS')
`, module, root, mode)
				cmd.Env = []string{"PATH=/usr/bin:/bin", "PYTHONDONTWRITEBYTECODE=1"}
				out, err := cmd.CombinedOutput()
				if err != nil || string(out) != "PROBE_PASS\n" {
					t.Fatalf("probe %s: %v %s", mode, err, out)
				}
			}
			probe("held")
			if err := held.Close(); err != nil {
				t.Fatal(err)
			}
			probe("released")
			probe("released")
			again, err := lock(root)
			if err != nil {
				t.Fatal(err)
			}
			defer again.Close()
			probe("held")
			after, err := os.Stat(path)
			if err != nil || !os.SameFile(before, after) || after.Size() != 0 || !before.ModTime().Equal(after.ModTime()) {
				t.Fatal("probe changed lock")
			}
			entries, err := os.ReadDir(root)
			if err != nil || len(entries) != 1 || entries[0].Name() != "writer.dev.lock" {
				t.Fatal("unexpected home mutation")
			}
		})
	}
}
