.PHONY: bootstrap build test scaffold vectors
bootstrap:
	cd web && npm ci --ignore-scripts
build:
	cd chain && GOTOOLCHAIN=local go build -mod=readonly ./...
	cd exchange && cargo build --locked
	cd web && npm run build
test: build
	cd chain && GOTOOLCHAIN=local go test -mod=readonly ./...
	cd exchange && cargo test --locked
	cd web && npm test
	python3 tests/test_vector_gate.py
	python3 tests/test_runtime.py
scaffold: bootstrap test
vectors:
	python3 ops/ci/vectors.py --repo . --config ops/ci/manifest.json --output .evidence/vectors.json
