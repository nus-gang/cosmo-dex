# NUS-17 S0-H reproduction

QA independently reproduced the pinned S0-G inputs at `248b9529cfb297b8d8060ef9f923a32cab2a6e3a`. **S0 gate FAIL; product T01–T16 NOT_RUN.** See `REPORT.md` and `TRACEABILITY.md`. An audit exit 0 means the evidence is internally consistent; it does not override the failing policy comparison.

Use a fresh clone of this branch, with full history. Do not reuse materialized component directories:

```sh
python3 security/prepare.py
bash security/run.sh
# Current rc2 inputs: exit 1, 420 comparisons / 413 matched / 7 differences.
python3 qa/audit.py
(cd web && npm run test:browser)
```

Go 1.24.4, Rust 1.92.0, Node 24.21.0, Python 3.14.0; locked dependencies. Set `CHROME_BIN` to the installed Chrome executable if needed. On this macOS runner Rust had no default: explicit `RUSTUP_HOME=/Users/gangdongju/.rustup RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin` selected the installed 1.92.0 without changing global settings. Use local equivalents on another machine. Local run used isolated `CARGO_HOME`, `GOCACHE`, `GOMODCACHE`, `npm_config_cache` beneath `qa/`; cache paths do not select source versions.

The B scaffold is separately archived at `cec18c78f41aea8c2c2935bf9ade6aa0fb34f3e9`; run `python3 tests/test_vector_gate.py`, `python3 tests/test_runtime.py` and its documented vector command there. Do not overlay B's placeholder components onto the C/D/E/F inputs. Its full manifest still exits 2 for six missing inputs.

Raw local logs, browser screenshot/JSON, exact source/document revisions, public synthetic signatures, CI artifacts and checksums are attached to the Paperclip issue. Dependency caches and credentials are excluded. The existing CI evidence is downloaded from run 36607517804, tested harness SHA `2be2347632909e1b6a417b3823220d5abcba4745`; `248b952` only adds documentation/evidence. No claim is made that the later QA report commit itself ran in that CI.
