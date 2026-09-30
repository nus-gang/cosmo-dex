#!/bin/sh
set -eu
export GOTOOLCHAIN=local
[ "$(go env GOVERSION)" = go1.26.5 ] || { echo 'Go 1.26.5 required' >&2; exit 1; }
[ "$(go list -m -f '{{.Version}}' github.com/cosmos/cosmos-sdk)" = v0.55.0 ]
[ "$(go list -m -f '{{.Version}}' github.com/cometbft/cometbft)" = v0.40.0 ]
go mod verify
