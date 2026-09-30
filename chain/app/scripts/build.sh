#!/bin/sh
set -eu
export GOTOOLCHAIN=local
sh scripts/check-toolchain.sh
mkdir -p bin
execution_sha=$(git rev-parse HEAD)
go build -mod=readonly -trimpath -buildvcs=false -ldflags "-X main.buildCommit=$execution_sha" -o bin/nusd ./cmd/nusd
