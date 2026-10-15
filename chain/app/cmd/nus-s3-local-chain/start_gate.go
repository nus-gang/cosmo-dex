//go:build dev_local_demo

package main

import (
	"context"
	"errors"
	"io"
	"time"
)

// awaitStart owns and closes input. IPC is sequencing, not organizational approval.
// The managed parent must freshly verify the exact candidate before START+EOF.
func awaitStart(ctx context.Context, input io.ReadCloser, output io.Writer, timeout time.Duration) error {
	defer input.Close()
	if timeout <= 0 || timeout > 30*time.Second {
		return errors.New("START_TIMEOUT_BOUND")
	}
	if ctx.Err() != nil {
		return errors.New("START_CANCELLED")
	}
	if _, e := io.WriteString(output, "CHAIN_READY\n"); e != nil {
		return errors.New("READY_WRITE_FAILED")
	}
	result := make(chan error, 1)
	go func() {
		raw, e := io.ReadAll(io.LimitReader(input, 7))
		if e != nil || string(raw) != "START\n" {
			result <- errors.New("START_REJECTED")
			return
		}
		result <- nil
	}()
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return errors.New("START_CANCELLED")
	case <-timer.C:
		return errors.New("START_TIMEOUT")
	case e := <-result:
		if ctx.Err() != nil {
			return errors.New("START_CANCELLED")
		}
		return e
	}
}
