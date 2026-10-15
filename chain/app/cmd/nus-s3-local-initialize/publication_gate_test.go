//go:build dev_local_demo

package main

import (
	"bytes"
	"context"
	"io"
	"testing"
	"time"
)

func TestPublicationGateExactEOFAndCancellation(t *testing.T) {
	for _, input := range []string{"PUBLISH\n", "", "START\n", "PUBLISH", "PUBLISH\nX"} {
		var ready bytes.Buffer
		err := awaitPublication(context.Background(), io.NopCloser(bytes.NewBufferString(input)), &ready, time.Second)
		if (err == nil) != (input == "PUBLISH\n") || ready.String() != "INITIALIZER_READY\n" {
			t.Fatal("gate protocol")
		}
	}
	reader, writer := io.Pipe()
	defer writer.Close()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var ready bytes.Buffer
	if awaitPublication(ctx, reader, &ready, time.Second) == nil || ready.Len() != 0 {
		t.Fatal("cancel")
	}
	reader, writer = io.Pipe()
	defer writer.Close()
	go func() { _, _ = writer.Write([]byte("PUBLISH\n")) }()
	if awaitPublication(context.Background(), reader, &ready, time.Millisecond*20) == nil {
		t.Fatal("missing EOF")
	}
}
