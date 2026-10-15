//go:build dev_local_demo

package localdirect

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	sdk "github.com/cosmos/cosmos-sdk/types"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
	"io"
	"os"
	"os/exec"
	"testing"
	"time"
)

type failedReader struct{}

func (failedReader) Read([]byte) (int, error) { return 0, io.ErrUnexpectedEOF }
func TestIPC(t *testing.T) {
	_, cfg := app.Encoding()
	k, e := mldsa65.GenPrivKeyFromSeed(bytes.Repeat([]byte{47}, 32))
	if e != nil {
		t.Fatal(e)
	}
	g := bytes.Repeat([]byte{1}, 32)
	raw := signed(t, cfg, &k, &ext.MsgDeposit{Owner: sdk.AccAddress(k.PubKey().Address()).String(), Denom: "DEVBASE", AmountAtoms: "1", RequestId: bytes.Repeat([]byte{2}, 32), ExpectedEpoch: "0", ExpiryHeight: "100", GenesisHash: g})
	r := Request{raw, k.PubKey().Address(), k.PubKey().Bytes(), g, "nus-s3-local-demo", "7", "3"}
	b, _ := json.Marshal(r)
	if target := os.Getenv("DIRECT_IPC_FIXTURE"); target != "" {
		f, err := os.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil { t.Fatal(err) }
		_, err = f.Write(b)
		closeErr := f.Close()
		if err != nil || closeErr != nil { t.Fatal("fixture output failed") }
	}
	var out bytes.Buffer
	if e := CheckIPC(cfg, bytes.NewReader(b), &out); e != nil {
		t.Fatal(e)
	}
	want := fmt.Sprintf("{\"version\":1,\"tx_sha256\":\"%x\",\"owner_bound\":true,\"broadcast\":false}\n", sha256.Sum256(raw))
	if binary := os.Getenv("DIRECT_IPC_BINARY"); binary != "" {
		c := exec.Command(binary, "verify", "--local-demo-profile", "nus-s3-local-demo", "--acknowledge-unproven-space")
		c.Stdin = bytes.NewReader(b)
		got, e := c.Output()
		if e != nil || string(got) != want {
			t.Fatal("executable binding", e)
		}
	}
	if out.String() != want {
		t.Fatal("unbound report")
	}
	cases := [][]byte{nil, b[:len(b)-1], append(bytes.Clone(b), '\n'), append(bytes.Clone(b), b...), bytes.Repeat([]byte{'x'}, MaxRequestBytes+1), bytes.Replace(b, []byte(`"sequence":"3"`), []byte(`"sequence":"03"`), 1), bytes.Replace(b, []byte(`"sequence":"3"`), []byte(`"sequence":"18446744073709551616"`), 1), bytes.Replace(b, []byte(`"sequence":"3"`), []byte(`"sequence":"3","sequence":"3"`), 1), bytes.Replace(b, []byte(`"sequence":"3"`), []byte(`"sequence":"3","unknown":1`), 1), bytes.Replace(b, []byte(`"account_number":"7"`), []byte(`"account_number":"8"`), 1)}
	for i, bad := range cases {
		out.Reset()
		if CheckIPC(cfg, bytes.NewReader(bad), &out) == nil || out.Len() != 0 {
			t.Fatalf("accepted %d", i)
		}
	}
	out.Reset()
	if CheckIPC(cfg, failedReader{}, &out) == nil || out.Len() != 0 {
		t.Fatal("IO accepted")
	}
}

func TestIPCExecutable(t *testing.T) {
	binary := os.Getenv("DIRECT_IPC_BINARY")
	if binary == "" {
		t.Skip("explicit compiled helper required")
	}
	args := []string{"verify", "--local-demo-profile", "nus-s3-local-demo", "--acknowledge-unproven-space"}
	for _, a := range [][]string{nil, {"verify"}, append(append([]string{}, args...), "--acknowledge-unproven-space"), args} {
		c := exec.Command(binary, a...)
		stdin, e := c.StdinPipe()
		if e != nil {
			t.Fatal(e)
		}
		var out, diag bytes.Buffer
		c.Stdout = &out
		c.Stderr = &diag
		start := time.Now()
		if e = c.Start(); e != nil {
			t.Fatal(e)
		}
		done := make(chan error, 1)
		go func() { done <- c.Wait() }()
		select {
		case e = <-done:
		case <-time.After(5 * time.Second):
			c.Process.Kill()
			<-done
			t.Fatal("deadline missing")
		}
		stdin.Close()
		if e == nil || c.ProcessState.ExitCode() != 2 || out.Len() != 0 || diag.String() != "DIRECT_TX_REJECTED\n" {
			t.Fatal("non-fixed refusal", diag.String())
		}
		if len(a) != len(args) && time.Since(start) > time.Second {
			t.Fatal("invalid args waited for stdin")
		}
	}
}
