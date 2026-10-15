//go:build dev_local_demo

package localkeys

import (
	"crypto/rand"
	"errors"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	"io"
)

// Authorities owns two operator seeds and one administrator seed. It performs
// no file IO. Call Destroy after private publication or on every failure path.
// Copies share revocation state; exported byte copies remain caller-owned.
// Clearing owned buffers is best effort, not a Go heap erasure guarantee.
type Authorities struct{ state *authorityState }
type authorityState struct {
	seeds  [3][32]byte
	public [3][]byte
	live   bool
}

func (Authorities) String() string   { return "localkeys.Authorities(REDACTED)" }
func (Authorities) GoString() string { return "localkeys.Authorities(REDACTED)" }
func (Authorities) MarshalJSON() ([]byte, error) {
	return nil, errors.New("PRIVATE_MATERIAL_SERIALIZATION_REJECTED")
}

// GenerateAuthorities has no caller seed input. Users retain their own tab keys.
func GenerateAuthorities() (*Authorities, error) { return generateAuthorities(rand.Reader) }
func generateAuthorities(entropy io.Reader) (*Authorities, error) {
	a := &Authorities{state: &authorityState{live: true}}
	ok := false
	defer func() {
		if !ok {
			a.Destroy()
		}
	}()
	seen := map[string]bool{}
	for i := range a.state.seeds {
		if _, err := io.ReadFull(entropy, a.state.seeds[i][:]); err != nil {
			return nil, errors.New("AUTHORITY_ENTROPY_FAILED")
		}
		key, err := mldsa65.GenPrivKeyFromSeed(a.state.seeds[i][:])
		if err != nil {
			return nil, errors.New("AUTHORITY_KEY_FAILED")
		}
		pub := key.PubKey()
		if pub == nil {
			clear(key.Key)
			return nil, errors.New("AUTHORITY_KEY_FAILED")
		}
		raw := append([]byte(nil), pub.Bytes()...)
		clear(key.Key)
		if len(raw) != 1952 || seen[string(raw)] {
			return nil, errors.New("AUTHORITY_DUPLICATE_KEY")
		}
		seen[string(raw)] = true
		a.state.public[i] = raw
	}
	ok = true
	return a, nil
}
func (a *Authorities) Public() ([][]byte, []byte, error) {
	if a == nil || a.state == nil || !a.state.live {
		return nil, nil, errors.New("AUTHORITY_CLOSED")
	}
	return [][]byte{append([]byte(nil), a.state.public[0]...), append([]byte(nil), a.state.public[1]...)}, append([]byte(nil), a.state.public[2]...), nil
}

// Seed returns exactly 32 bytes for private operator.seed/admin.seed publication.
// Role is fixed: 0/1 operators; 2 administrator. Never log returned bytes.
func (a *Authorities) Seed(role int) ([]byte, error) {
	if a == nil || a.state == nil || !a.state.live || role < 0 || role > 2 {
		return nil, errors.New("AUTHORITY_CLOSED")
	}
	return append([]byte(nil), a.state.seeds[role][:]...), nil
}
func (a *Authorities) Destroy() {
	if a == nil || a.state == nil {
		return
	}
	for i := range a.state.seeds {
		clear(a.state.seeds[i][:])
		clear(a.state.public[i])
		a.state.public[i] = nil
	}
	a.state.live = false
}
