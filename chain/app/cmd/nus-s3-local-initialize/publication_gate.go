//go:build dev_local_demo

package main

import (
	"context"
	"errors"
	"io"
	"time"
)

// No filesystem publication occurs before exact PUBLISH plus EOF. This IPC is
// not an approval: only initialize_cli.py is the supported L-T entry point.
func awaitPublication(ctx context.Context, input io.ReadCloser, output io.Writer, timeout time.Duration) error {
	defer input.Close()
	if timeout <= 0 || timeout > 30*time.Second || ctx.Err() != nil {
		return errors.New("PUBLICATION_GATE_REJECTED")
	}
	if _, err := io.WriteString(output, "INITIALIZER_READY\n"); err != nil {
		return errors.New("PUBLICATION_GATE_REJECTED")
	}
	result := make(chan bool, 1)
	go func() {
		raw, err := io.ReadAll(io.LimitReader(input, 9))
		result <- err == nil && string(raw) == "PUBLISH\n"
	}()
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	select {
	case ok := <-result:
		if ok && ctx.Err() == nil {
			return nil
		}
	case <-ctx.Done():
	case <-timer.C:
	}
	return errors.New("PUBLICATION_GATE_REJECTED")
}
