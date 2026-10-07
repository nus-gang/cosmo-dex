//go:build dev_local_demo

package localkeys

import (
	"errors"
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
	"testing"
)

func privateParent(t *testing.T) string {
	t.Helper()
	p, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err = os.Chmod(p, 0700); err != nil {
		t.Fatal(err)
	}
	return p
}
func TestAuthorityPublish(t *testing.T) {
	p := privateParent(t)
	for _, fee := range []string{"fee0", "fee25"} {
		a, err := GenerateAuthorities()
		if err != nil {
			t.Fatal(err)
		}
		expected := make([][]byte, 3)
		for i := range expected {
			expected[i], _ = a.Seed(i)
			defer clear(expected[i])
		}
		root := filepath.Join(p, fee)
		if err = a.Publish(root); err != nil {
			t.Fatal(err)
		}
		if _, err = a.Seed(0); err == nil {
			t.Fatal("seeds remain accessible")
		}
		for i, role := range []string{"operator-0", "operator-1", "administrator"} {
			dir := filepath.Join(root, role)
			name := "operator.seed"
			if i == 2 {
				name = "admin.seed"
			}
			raw, err := os.ReadFile(filepath.Join(dir, name))
			if err != nil || string(raw) != string(expected[i]) {
				t.Fatal("seed bytes")
			}
			clear(raw)
			for path, mode := range map[string]os.FileMode{root: 0700, dir: 0700, filepath.Join(dir, name): 0600} {
				s, e := os.Lstat(path)
				if e != nil || s.Mode().Perm() != mode {
					t.Fatal("mode")
				}
			}
		}
		again, _ := GenerateAuthorities()
		if again.Publish(root) == nil {
			t.Fatal("overwrite")
		}
		if _, err = again.Seed(0); err == nil {
			t.Fatal("failure did not consume")
		}
	}
}
func TestAuthorityPublishRejectsParentAndLinks(t *testing.T) {
	p := privateParent(t)
	link := filepath.Join(p, "link")
	if err := os.Symlink(p, link); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{filepath.Join(link, "keys"), "relative", filepath.Join(p, "missing", "keys")} {
		a, _ := GenerateAuthorities()
		if a.Publish(path) == nil {
			t.Fatal("bad parent accepted")
		}
	}
	os.Chmod(p, 0755)
	a, _ := GenerateAuthorities()
	if a.Publish(filepath.Join(p, "keys")) == nil {
		t.Fatal("public parent")
	}
	os.Chmod(p, 0700)
	if _, err := os.Stat(filepath.Join(p, "keys")); !os.IsNotExist(err) {
		t.Fatal("unexpected output")
	}
}
func TestAuthorityPartialPublishPreserved(t *testing.T) {
	p := privateParent(t)
	fd, err := unix.Open(p, unix.O_RDONLY|unix.O_DIRECTORY, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(fd)
	a, _ := GenerateAuthorities()
	defer a.Destroy()
	seeds := make([][]byte, 3)
	for i := range seeds {
		seeds[i], _ = a.Seed(i)
		defer clear(seeds[i])
	}
	calls := 0
	err = publishSeeds(fd, seeds, func(int) error { calls++; return errors.New("injected") })
	if err == nil || calls != 1 {
		t.Fatal("sync failure")
	}
	if b, e := os.ReadFile(filepath.Join(p, "operator-0", "operator.seed")); e != nil || len(b) != 32 {
		t.Fatal("partial evidence missing")
	}
	if _, e := os.Stat(filepath.Join(p, "operator-1")); !os.IsNotExist(e) {
		t.Fatal("continued after failure")
	}
	if publishSeeds(fd, seeds, unix.Fsync) == nil {
		t.Fatal("retry accepted")
	}
}
