#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
export GOTOOLCHAIN=local
export GOMODCACHE="${GOMODCACHE:-$PWD/.cache/mod}"
export GOCACHE="${GOCACHE:-$PWD/.cache/build}"
go test -mod=readonly -count=1 -json ./...
