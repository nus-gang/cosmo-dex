#!/bin/sh
set -eu
# Run from chain/app. protoc 29.3 is a build tool, not a runtime dependency.
: "${PROTOC:?set PROTOC to the protoc 29.3 executable}"
[ "$("$PROTOC" --version)" = 'libprotoc 29.3' ]
export GOTOOLCHAIN=local
[ "$(go env GOVERSION)" = go1.26.5 ]
S3_PROTO_SCRATCH=$(mktemp -d "${PAPERCLIP_RUN_SCRATCH_DIR:-${TMPDIR:-/tmp}}/nus-s3-proto.XXXXXX")
trap 'rm -rf "$S3_PROTO_SCRATCH"' EXIT HUP INT TERM
go build -mod=readonly -o "$S3_PROTO_SCRATCH/protoc-gen-gogo" github.com/cosmos/gogoproto/protoc-gen-gogo
S3_SDK_DIR=$(go list -m -f '{{.Dir}}' github.com/cosmos/cosmos-sdk)
"$PROTOC" -I proto -I "$S3_SDK_DIR/proto" -I "$(dirname "$PROTOC")/../include" \
  --plugin="protoc-gen-gogo=$S3_PROTO_SCRATCH/protoc-gen-gogo" \
  --gogo_out=plugins=grpc,paths=source_relative:"$S3_PROTO_SCRATCH" nus/exchange/s3/v1/tx.proto
cp "$S3_PROTO_SCRATCH/nus/exchange/s3/v1/tx.pb.go" x/exchange/s3types/tx.pb.go
