#!/usr/bin/env bash
# Requires materialized inputs (prepare.py), locked dependencies and toolchains.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p security/evidence exchange/examples
export GOTOOLCHAIN=local
export GOCACHE="${GOCACHE:-$PWD/security/.go-build}"
export GOMODCACHE="${GOMODCACHE:-$PWD/security/.go-mod}"
python3 protocol/v1/tools/check.py > security/evidence/protocol-check.txt 2>&1
bash chain/test.sh > security/evidence/go-tests.jsonl 2>&1
"${CARGO:-cargo}" test --manifest-path exchange/Cargo.toml --locked > security/evidence/rust-tests.txt 2>&1
python3 settlement/v1/test_contract.py > security/evidence/settlement-tests.txt 2>&1
python3 security/settlement_review.py
(cd web && npm ci --ignore-scripts && npm test && npm run build) > security/evidence/ts-run.txt 2>&1
cp security/runner.rs exchange/examples/security.rs
(cd chain && go build -mod=readonly -o ../security/go-runner ../security/go.go)
"${CARGO:-cargo}" build --manifest-path exchange/Cargo.toml --locked --example security > security/evidence/harness-build.txt 2>&1
# Exit 1 means unresolved policy differences; do not turn it into a PASS.
python3 security/review.py > security/evidence/review-stdout.txt 2>&1
