//go:build dev_local_demo

package localkeys

import (
	"errors"
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
)

// Publish consumes authority seeds on success AND failure. Only a new directory
// beneath an existing canonical, uid-owned 0700 parent is accepted. Partial
// output is evidence: it is never removed, overwritten, or retried in place.
// No genesis/guard validation, service activation, or heap erasure guarantee.
func (a *Authorities) Publish(path string) error {
	defer a.Destroy()
	seeds := make([][]byte, 3)
	defer func() {
		for _, seed := range seeds {
			clear(seed)
		}
	}()
	for i := range seeds {
		var err error
		seeds[i], err = a.Seed(i)
		if err != nil {
			return errors.New("AUTHORITY_CLOSED")
		}
	}
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return errors.New("KEY_PARENT")
	}
	parent := filepath.Dir(path)
	canonical, err := filepath.EvalSymlinks(parent)
	if err != nil || canonical != parent {
		return errors.New("KEY_PARENT")
	}
	fd, err := unix.Open(parent, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return errors.New("KEY_PARENT")
	}
	defer unix.Close(fd)
	var st unix.Stat_t
	if unix.Fstat(fd, &st) != nil || st.Uid != uint32(os.Geteuid()) || st.Mode&07777 != 0700 {
		return errors.New("KEY_PARENT")
	}
	name := filepath.Base(path)
	if unix.Mkdirat(fd, name, 0700) != nil {
		return errors.New("KEY_NEW_ROOT_REQUIRED")
	}
	if unix.Fsync(fd) != nil {
		return errors.New("KEY_SYNC")
	}
	root, err := unix.Openat(fd, name, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return errors.New("KEY_ROOT")
	}
	defer unix.Close(root)
	var rootStat, parentNow unix.Stat_t
	if unix.Fstat(root, &rootStat) != nil || rootStat.Uid != uint32(os.Geteuid()) || rootStat.Mode&07777 != 0700 || unix.Lstat(parent, &parentNow) != nil || parentNow.Dev != st.Dev || parentNow.Ino != st.Ino {
		return errors.New("KEY_ROOT_CHANGED")
	}
	if err := publishSeeds(root, seeds, unix.Fsync); err != nil {
		return err
	}
	if unix.Fsync(fd) != nil {
		return errors.New("KEY_SYNC")
	}
	var opened, current unix.Stat_t
	if unix.Fstat(root, &opened) != nil || unix.Lstat(path, &current) != nil || opened.Dev != current.Dev || opened.Ino != current.Ino {
		return errors.New("KEY_ROOT_CHANGED")
	}
	return nil
}

func publishSeeds(root int, seeds [][]byte, sync func(int) error) error {
	for i, role := range []string{"operator-0", "operator-1", "administrator"} {
		if unix.Mkdirat(root, role, 0700) != nil {
			return errors.New("KEY_ROLE")
		}
		dir, err := unix.Openat(root, role, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			return errors.New("KEY_ROLE")
		}
		err = func() error {
			defer unix.Close(dir)
			name := "operator.seed"
			if i == 2 {
				name = "admin.seed"
			}
			fd, err := unix.Openat(dir, name, unix.O_WRONLY|unix.O_CREAT|unix.O_EXCL|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0600)
			if err != nil {
				return errors.New("KEY_FILE")
			}
			defer unix.Close(fd)
			raw := seeds[i]
			for len(raw) > 0 {
				n, err := unix.Write(fd, raw)
				if err != nil || n <= 0 {
					return errors.New("KEY_WRITE")
				}
				raw = raw[n:]
			}
			if sync(fd) != nil || sync(dir) != nil {
				return errors.New("KEY_SYNC")
			}
			return nil
		}()
		if err != nil {
			return err
		}
	}
	if sync(root) != nil {
		return errors.New("KEY_SYNC")
	}
	return nil
}
