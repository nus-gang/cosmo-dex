//go:build dev_local_demo

package main

import (
	"bytes"
	"context"
	"errors"
	"io"
	"strings"
	"testing"
	"time"
)

type trackedInput struct {
	io.Reader
	closed bool
}

func (r *trackedInput) Close() error { r.closed = true; return nil }

type brokenWriter struct{}

func (brokenWriter) Write([]byte) (int, error) { return 0, errors.New("injected") }

func TestStartGateExactFrame(t *testing.T) {
	for _, raw := range []string{"START\n", "", "START", "START\nX", "START\nSTART\n", "STOP\n"} {
		in := &trackedInput{Reader: strings.NewReader(raw)}
		var out bytes.Buffer
		e := awaitStart(context.Background(), in, &out, time.Second)
		if (e == nil) != (raw == "START\n") || !in.closed || out.String() != "CHAIN_READY\n" {
			t.Fatalf("frame %q: %v", raw, e)
		}
	}
}
func TestStartGateTimeoutAndCancellation(t *testing.T) {
	for _, cancelNow := range []bool{false, true} {
		r, w := io.Pipe()
		ctx, cancel := context.WithCancel(context.Background())
		if cancelNow {
			cancel()
		}
		var out bytes.Buffer
		e := awaitStart(ctx, r, &out, 10*time.Millisecond)
		cancel()
		want := "START_TIMEOUT"
		if cancelNow {
			want = "START_CANCELLED"
			if out.Len() != 0 {
				t.Fatal("ready after cancel")
			}
		}
		if e == nil || e.Error() != want {
			t.Fatal(e)
		}
		if _, e = w.Write([]byte("START\n")); e == nil {
			t.Fatal("input not closed")
		}
		w.Close()
	}
}
func TestStartGateOutputFailureAndBounds(t *testing.T) {
	for _, d := range []time.Duration{0, 31 * time.Second, time.Second} {
		in := &trackedInput{Reader: strings.NewReader("START\n")}
		if e := awaitStart(context.Background(), in, brokenWriter{}, d); e == nil || !in.closed {
			t.Fatal(e)
		}
	}
}
