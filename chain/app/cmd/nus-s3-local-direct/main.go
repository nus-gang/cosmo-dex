//go:build dev_local_demo

// Offline signature helper. No node, home, listener, key file or broadcast.
package main

import (
	"fmt"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	"github.com/nus-gang/cosmo-dex/chain/app/internal/localdirect"
	"os"
	"reflect"
	"time"
)

func main() {
	reject := func() { fmt.Fprintln(os.Stderr, "DIRECT_TX_REJECTED"); os.Exit(2) }
	if !reflect.DeepEqual(os.Args[1:], []string{"verify", "--local-demo-profile", "nus-s3-local-demo", "--acknowledge-unproven-space"}) {
		reject()
	}
	timer := time.AfterFunc(3*time.Second, reject)
	defer timer.Stop()
	_, cfg := app.Encoding()
	if localdirect.CheckIPC(cfg, os.Stdin, os.Stdout) != nil {
		reject()
	}
}
