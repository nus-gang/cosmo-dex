//go:build dev_local_demo

package localdirect

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"github.com/cosmos/cosmos-sdk/client"
	"io"
	"strconv"
)

// Request is canonical JSON produced by json.Marshal, followed by EOF.
// Decimal integers are strings so no transport float conversion can occur.
type Request struct {
	Raw           []byte `json:"raw"`
	Owner         []byte `json:"owner"`
	PublicKey     []byte `json:"public_key"`
	Genesis       []byte `json:"genesis"`
	ChainID       string `json:"chain_id"`
	AccountNumber string `json:"account_number"`
	Sequence      string `json:"sequence"`
}

const MaxRequestBytes = 192 * 1024

// CheckIPC is a single finite request; caller must impose a read deadline.
// No response bytes are written until complete decoding and signature checking.
func CheckIPC(cfg client.TxConfig, in io.Reader, out io.Writer) error {
	raw, err := io.ReadAll(io.LimitReader(in, MaxRequestBytes+1))
	if err != nil || len(raw) == 0 || len(raw) > MaxRequestBytes {
		return fmt.Errorf("DIRECT_TX_REJECTED")
	}
	var r Request
	if json.Unmarshal(raw, &r) != nil {
		return fmt.Errorf("DIRECT_TX_REJECTED")
	}
	canonical, err := json.Marshal(r)
	if err != nil || !bytes.Equal(raw, canonical) {
		return fmt.Errorf("DIRECT_TX_REJECTED")
	}
	n, err := strconv.ParseUint(r.AccountNumber, 10, 64)
	if err != nil || strconv.FormatUint(n, 10) != r.AccountNumber {
		return fmt.Errorf("DIRECT_TX_REJECTED")
	}
	s, err := strconv.ParseUint(r.Sequence, 10, 64)
	if err != nil || strconv.FormatUint(s, 10) != r.Sequence {
		return fmt.Errorf("DIRECT_TX_REJECTED")
	}
	if err = Verify(cfg, r.Raw, r.Owner, r.PublicKey, r.Genesis, r.ChainID, n, s); err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "{\"version\":1,\"tx_sha256\":\"%x\",\"owner_bound\":true,\"broadcast\":false}\n", sha256.Sum256(r.Raw))
	return err
}
